pub mod bans;
pub mod diff;
pub mod doc;
pub mod edit;
pub mod enums;
pub mod external_moderation;
pub mod filter;
pub mod inbox;
pub mod moderation;
pub mod naming;
pub mod notices;
pub mod prompts;
pub mod restrictions;
pub mod secrets;
pub mod sketches;
pub mod slice;
pub mod split;
pub mod templates;

use chrono::SecondsFormat;

/// One clock, one format: ISO-8601 UTC, which SQLite sorts correctly as text.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}
