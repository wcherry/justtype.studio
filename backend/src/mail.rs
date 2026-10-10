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
//! Two ways to send. `Outbox` appends each message as a line of JSON to a
//! file, which is what development, the smoke walkthrough and the tests use —
//! the link in a message can be read back and followed, so the whole flow is
//! exercised without a mail server. `Smtp` hands it to a real one, configured
//! from the environment; which provider sits behind it is a deployment choice,
//! not this code's.
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
}

impl Mailer {
    /// From `JUSTTYPE_MAIL`: `outbox` (the default) or `smtp`.
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
            other => anyhow::bail!("JUSTTYPE_MAIL must be `outbox` or `smtp`, not `{other}`"),
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
}
