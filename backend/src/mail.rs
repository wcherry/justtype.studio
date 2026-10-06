//! Mail going out: proving an address, resetting a password.
//!
//! Two ways to send. `Outbox` appends each message as a line of JSON to a
//! file, which is what development, the smoke walkthrough and the tests use —
//! the link in a message can be read back and followed, so the whole flow is
//! exercised without a mail server. `Smtp` hands it to a real one, configured
//! from the environment; which provider sits behind it is a deployment choice,
//! not this code's.
use std::path::PathBuf;

use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use serde::Serialize;

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub to: String,
    pub subject: String,
    pub body: String,
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
                let mut line = serde_json::to_string(&serde_json::json!({
                    "at": crate::domain::now_iso(),
                    "to": message.to,
                    "subject": message.subject,
                    "body": message.body,
                }))?;
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
                let email = lettre::Message::builder()
                    .from(from.parse().map_err(|e| {
                        ApiError::internal(format!("JUSTTYPE_MAIL_FROM is not an address: {e}"))
                    })?)
                    .to(message
                        .to
                        .parse()
                        .map_err(|_| ApiError::BadRequest("that is not an email address".into()))?)
                    .subject(message.subject)
                    .header(ContentType::TEXT_PLAIN)
                    .body(message.body)
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
