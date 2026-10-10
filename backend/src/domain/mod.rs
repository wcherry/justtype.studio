pub mod diff;
pub mod doc;
pub mod edit;
pub mod enums;
pub mod inbox;
pub mod naming;
pub mod prompts;
pub mod sketches;
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
