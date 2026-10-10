//! Mail going out: proving an address, resetting a password, the daily
//! prompt.
//!
//! Mail sent as part of a request (a reset link) goes straight out. Mail a
//! scheduled job decides to send is put in `mail_queue` inside the same
//! transaction as the record of having sent it ([`enqueue`]), and the job
//! runner delivers the queue ([`deliver_due`]), retrying with backoff until
//! the message's `give_up_at`. A send can then never be recorded without its
//! message, or the other way round.
//!
//! Three ways to send. `Outbox` appends each message as a line of JSON to a
//! file, which is what development, the smoke walkthrough and the tests use —
//! the link in a message can be read back and followed, so the whole flow is
//! exercised without a mail server. `Smtp` hands it to a real one, configured
//! from the environment. `Resend` posts it to Resend's HTTP API with an API
//! key, for when there is no relay to speak SMTP to.
use std::path::PathBuf;

use diesel::prelude::*;
use lettre::message::header::{ContentType, HeaderName, HeaderValue};
use lettre::message::MultiPart;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use serde::Serialize;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Default, Serialize)]
pub struct Message {
    pub to: String,
    pub subject: String,
    /// The plain-text part, always present.
    pub body: String,
    /// An HTML alternative, when there is one.
    pub html: Option<String>,
    /// Extra headers, such as `List-Unsubscribe`.
    pub headers: Vec<(String, String)>,
}

#[derive(Clone)]
pub enum Mailer {
    Outbox(PathBuf),
    Smtp {
        transport: AsyncSmtpTransport<Tokio1Executor>,
        from: String,
    },
    Resend {
        client: reqwest::Client,
        api_key: String,
        /// `https://api.resend.com` unless `JUSTTYPE_RESEND_URL` says otherwise.
        base_url: String,
        from: String,
    },
}

/// What Resend says when it refuses a message.
#[derive(serde::Deserialize)]
struct ResendError {
    #[serde(default)]
    name: String,
    #[serde(default)]
    message: String,
}

impl Mailer {
    /// From `JUSTTYPE_MAIL`: `outbox` (the default), `smtp` or `resend`.
    pub fn from_env(data_dir: &std::path::Path) -> anyhow::Result<Self> {
        let kind = std::env::var("JUSTTYPE_MAIL").unwrap_or_else(|_| "outbox".into());
        match kind.as_str() {
            "outbox" => {
                let path = std::env::var("JUSTTYPE_OUTBOX")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| data_dir.parent().unwrap_or(data_dir).join("outbox.jsonl"));
                tracing::info!(path = %path.display(), "mail goes to the outbox file, not to anyone");
                Ok(Mailer::Outbox(path))
            }
            "smtp" => {
                let var = |key: &str| {
                    std::env::var(key).map_err(|_| anyhow::anyhow!("{key} is required for SMTP"))
                };
                let host = var("JUSTTYPE_SMTP_HOST")?;
                let from = var("JUSTTYPE_MAIL_FROM")?;
                let mut builder = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)?;
                if let Ok(port) = std::env::var("JUSTTYPE_SMTP_PORT") {
                    builder = builder.port(port.parse()?);
                }
                if let (Ok(user), Ok(pass)) = (
                    std::env::var("JUSTTYPE_SMTP_USER"),
                    std::env::var("JUSTTYPE_SMTP_PASSWORD"),
                ) {
                    builder = builder.credentials(Credentials::new(user, pass));
                }
                Ok(Mailer::Smtp {
                    transport: builder.build(),
                    from,
                })
            }
            "resend" => {
                let var = |key: &str| {
                    std::env::var(key).map_err(|_| anyhow::anyhow!("{key} is required for Resend"))
                };
                let api_key = var("JUSTTYPE_RESEND_API_KEY")?;
                let from = var("JUSTTYPE_MAIL_FROM")?;
                let base_url = std::env::var("JUSTTYPE_RESEND_URL")
                    .unwrap_or_else(|_| "https://api.resend.com".into())
                    .trim_end_matches('/')
                    .to_string();
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()?;
                Ok(Mailer::Resend {
                    client,
                    api_key,
                    base_url,
                    from,
                })
            }
            other => {
                anyhow::bail!("JUSTTYPE_MAIL must be `outbox`, `smtp` or `resend`, not `{other}`")
            }
        }
    }

    pub async fn send(&self, message: Message) -> ApiResult<()> {
        match self {
            Mailer::Outbox(path) => {
                use tokio::io::AsyncWriteExt;
                let mut record = serde_json::json!({
                    "at": crate::domain::now_iso(),
                    "to": message.to,
                    "subject": message.subject,
                    "body": message.body,
                });
                if let Some(html) = &message.html {
                    record["html"] = html.clone().into();
                }
                if !message.headers.is_empty() {
                    record["headers"] = message
                        .headers
                        .iter()
                        .map(|(k, v)| (k.clone(), serde_json::Value::from(v.clone())))
                        .collect::<serde_json::Map<_, _>>()
                        .into();
                }
                let mut line = serde_json::to_string(&record)?;
                line.push('\n');
                if let Some(parent) = path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                let mut file = tokio::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .await?;
                file.write_all(line.as_bytes()).await?;
                Ok(())
            }
            Mailer::Smtp { transport, from } => {
                let mut builder = lettre::Message::builder()
                    .from(from.parse().map_err(|e| {
                        ApiError::internal(format!("JUSTTYPE_MAIL_FROM is not an address: {e}"))
                    })?)
                    .to(message
                        .to
                        .parse()
                        .map_err(|_| ApiError::BadRequest("that is not an email address".into()))?)
                    .subject(message.subject);
                for (name, value) in message.headers {
                    let name = HeaderName::new_from_ascii(name)
                        .map_err(|e| ApiError::internal(format!("a bad header name: {e}")))?;
                    builder = builder.raw_header(HeaderValue::new(name, value));
                }
                let built = match message.html {
                    Some(html) => {
                        builder.multipart(MultiPart::alternative_plain_html(message.body, html))
                    }
                    None => builder.header(ContentType::TEXT_PLAIN).body(message.body),
                };
                let email = built
                    .map_err(|e| ApiError::internal(format!("could not build the message: {e}")))?;
                transport
                    .send(email)
                    .await
                    .map_err(|e| ApiError::internal(format!("the mail server refused it: {e}")))?;
                Ok(())
            }
            Mailer::Resend {
                client,
                api_key,
                base_url,
                from,
            } => {
                let mut body = serde_json::json!({
                    "from": from,
                    "to": [message.to],
                    "subject": message.subject,
                    "text": message.body,
                });
                if let Some(html) = message.html {
                    body["html"] = html.into();
                }
                if !message.headers.is_empty() {
                    body["headers"] = message
                        .headers
                        .into_iter()
                        .map(|(k, v)| (k, serde_json::Value::from(v)))
                        .collect::<serde_json::Map<_, _>>()
                        .into();
                }
                let response = client
                    .post(format!("{base_url}/emails"))
                    .bearer_auth(api_key)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| ApiError::internal(format!("could not reach Resend: {e}")))?;
                let status = response.status();
                if status.is_success() {
                    return Ok(());
                }
                let detail = response.bytes().await.unwrap_or_default();
                let detail = serde_json::from_slice::<ResendError>(&detail)
                    .map(|e| format!("{}: {}", e.name, e.message))
                    .unwrap_or_else(|_| {
                        String::from_utf8_lossy(&detail).chars().take(300).collect()
                    });
                Err(ApiError::internal(format!(
                    "Resend refused it ({status}): {detail}"
                )))
            }
        }
    }
}

// --- the queue -----------------------------------------------------------

/// Puts `message` in the queue, to be delivered by the job runner. Call inside
/// the transaction that records why it is being sent.
pub fn enqueue(
    conn: &mut crate::db::Conn,
    message: &Message,
    give_up_at: chrono::DateTime<chrono::Utc>,
) -> ApiResult<()> {
    use crate::db::models::NewQueuedMail;
    use crate::db::schema::mail_queue;
    let now = crate::clock::now_iso();
    diesel::insert_into(mail_queue::table)
        .values(&NewQueuedMail {
            id: crate::domain::new_id(),
            to_address: message.to.clone(),
            subject: message.subject.clone(),
            body: message.body.clone(),
            html: message.html.clone(),
            headers: (!message.headers.is_empty())
                .then(|| serde_json::to_string(&message.headers))
                .transpose()?,
            created_at: now.clone(),
            attempts: 0,
            next_attempt_at: now,
            give_up_at: give_up_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            sent_at: None,
            failed_at: None,
            last_error: None,
        })
        .execute(conn)?;
    Ok(())
}

/// How long to wait after the `attempts`th failure: a minute, doubling, at
/// most an hour.
pub fn backoff(attempts: i32) -> chrono::Duration {
    let minutes = 1i64 << attempts.clamp(0, 6);
    chrono::Duration::minutes(minutes.min(60))
}

/// Sends everything due, one at a time. Returns how many went.
pub async fn deliver_due(pool: &crate::db::Pool, mailer: &Mailer) -> ApiResult<usize> {
    use crate::db::models::QueuedMail;
    use crate::db::schema::mail_queue;
    let now = crate::clock::now_iso();
    let due: Vec<QueuedMail> = crate::db::blocking(pool, move |conn| {
        Ok(mail_queue::table
            .filter(mail_queue::sent_at.is_null())
            .filter(mail_queue::failed_at.is_null())
            .filter(mail_queue::next_attempt_at.le(&now))
            .order(mail_queue::created_at.asc())
            .limit(200)
            .select(QueuedMail::as_select())
            .load(conn)?)
    })
    .await?;
    let mut sent = 0;
    for row in due {
        let message = Message {
            to: row.to_address.clone(),
            subject: row.subject.clone(),
            body: row.body.clone(),
            html: row.html.clone(),
            headers: row
                .headers
                .as_deref()
                .and_then(|h| serde_json::from_str(h).ok())
                .unwrap_or_default(),
        };
        let outcome = mailer.send(message).await;
        let id = row.id.clone();
        let ok = outcome.is_ok();
        crate::db::blocking(pool, move |conn| {
            let now = crate::clock::now();
            let at = |t: chrono::DateTime<chrono::Utc>| {
                t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            };
            match outcome {
                Ok(()) => {
                    diesel::update(mail_queue::table.find(&id))
                        .set((
                            mail_queue::sent_at.eq(at(now)),
                            mail_queue::attempts.eq(row.attempts + 1),
                        ))
                        .execute(conn)?;
                }
                Err(e) => {
                    let attempts = row.attempts + 1;
                    let next = now + backoff(attempts);
                    let give_up = chrono::DateTime::parse_from_rfc3339(&row.give_up_at)
                        .map(|t| t.with_timezone(&chrono::Utc))
                        .unwrap_or(now);
                    tracing::warn!(to = %row.to_address, error = %e, attempts, "mail not sent");
                    diesel::update(mail_queue::table.find(&id))
                        .set((
                            mail_queue::attempts.eq(attempts),
                            mail_queue::last_error.eq(e.to_string()),
                            mail_queue::next_attempt_at.eq(at(next)),
                            mail_queue::failed_at.eq((next > give_up).then(|| at(now))),
                        ))
                        .execute(conn)?;
                }
            }
            Ok(())
        })
        .await?;
        if ok {
            sent += 1;
        }
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_back_off_to_an_hour() {
        assert_eq!(backoff(1), chrono::Duration::minutes(2));
        assert_eq!(backoff(3), chrono::Duration::minutes(8));
        assert_eq!(backoff(20), chrono::Duration::minutes(60));
    }

    /// A stand-in for Resend that records what it was sent and answers with
    /// `status`.
    async fn fake_resend(
        status: u16,
    ) -> (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>>,
    ) {
        use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
        type Seen = std::sync::Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>>;
        let seen: Seen = Default::default();
        let app = Router::new()
            .route(
                "/emails",
                post(
                    move |State(seen): State<Seen>,
                          headers: HeaderMap,
                          Json(body): Json<serde_json::Value>| async move {
                        let auth = headers
                            .get("authorization")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or_default()
                            .to_string();
                        seen.lock().unwrap().push((auth, body));
                        let code = axum::http::StatusCode::from_u16(status).unwrap();
                        if code.is_success() {
                            (code, Json(serde_json::json!({ "id": "abc" })))
                        } else {
                            (
                                code,
                                Json(serde_json::json!({
                                    "statusCode": status,
                                    "name": "validation_error",
                                    "message": "The domain is not verified.",
                                })),
                            )
                        }
                    },
                ),
            )
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, seen)
    }

    fn resend(base_url: String) -> Mailer {
        Mailer::Resend {
            client: reqwest::Client::new(),
            api_key: "re_test".into(),
            base_url,
            from: "Just Type <hello@example.com>".into(),
        }
    }

    #[tokio::test]
    async fn resend_gets_the_whole_message() {
        let (url, seen) = fake_resend(200).await;
        resend(url)
            .send(Message {
                to: "ada@example.com".into(),
                subject: "Today's prompt".into(),
                body: "plain".into(),
                html: Some("<p>html</p>".into()),
                headers: vec![("List-Unsubscribe".into(), "<https://x/u>".into())],
            })
            .await
            .unwrap();
        let seen = seen.lock().unwrap();
        let (auth, body) = &seen[0];
        assert_eq!(auth, "Bearer re_test");
        assert_eq!(body["from"], "Just Type <hello@example.com>");
        assert_eq!(body["to"], serde_json::json!(["ada@example.com"]));
        assert_eq!(body["subject"], "Today's prompt");
        assert_eq!(body["text"], "plain");
        assert_eq!(body["html"], "<p>html</p>");
        assert_eq!(body["headers"]["List-Unsubscribe"], "<https://x/u>");
    }

    #[tokio::test]
    async fn a_refusal_says_why() {
        let (url, _) = fake_resend(403).await;
        let err = resend(url)
            .send(Message {
                to: "ada@example.com".into(),
                subject: "s".into(),
                body: "b".into(),
                ..Default::default()
            })
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("The domain is not verified."), "{err}");
    }
}
