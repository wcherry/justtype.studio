use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub data_dir: PathBuf,
    pub jwt_secret: String,
    pub access_ttl_minutes: i64,
    pub refresh_ttl_days: i64,
    pub cors_origins: Vec<String>,
    /// Per user, per minute. Zero turns the limit off.
    pub comment_rate: i64,
    pub vote_rate: i64,
    pub reaction_rate: i64,
    /// Tries at a second factor per account per minute: login codes, turning
    /// it on or off, deleting the account.
    pub second_factor_rate: i64,
    /// Where the web client lives, for the links in mail.
    pub public_url: String,
    /// Where backups go. Unset, the status check says nothing is being kept.
    pub backup_dir: Option<PathBuf>,
    /// How many backups to keep.
    pub backup_keep: usize,
    /// `json` for one JSON object per log line; anything else is for people.
    pub log_format: String,
    /// Bearer token for `/api/metrics`. Unset, that endpoint does not exist.
    pub metrics_token: Option<String>,
    /// Memory for snapshot scene bodies, in megabytes.
    pub cache_mb: usize,
    /// Requests per minute from one address, to anything. Zero turns it off.
    pub ip_rate: i64,
    /// Requests per minute from one address to the sign-in routes.
    pub auth_ip_rate: i64,
    /// Behind a reverse proxy, the client's address is the last one the proxy
    /// added to X-Forwarded-For. Only set this when there is such a proxy: a
    /// client can put anything it likes in that header.
    pub trust_proxy: bool,
    /// For summaries. Unset, the feature is off and says so.
    pub anthropic_api_key: Option<String>,
    pub anthropic_url: String,
    pub summary_model: String,
    pub assistant_model: String,
    /// A built web app (`web/dist`) to serve beside the API, as the Docker
    /// image does. Unset in development, where Vite serves it.
    pub web_dir: Option<PathBuf>,
}

/// The old name for every setting, before the product was renamed.
const LEGACY_PREFIX: &str = "SCRIPTORIUM_";
const PREFIX: &str = "JUSTTYPE_";

/// Gives every `SCRIPTORIUM_*` setting its `JUSTTYPE_*` name, where the new
/// name is not set itself, so an environment or `.env` written before the
/// rename keeps working. Returns the old names it found, to warn about once
/// logging is up. Call before anything reads the environment.
pub fn adopt_legacy_env() -> Vec<String> {
    let mut adopted = Vec::new();
    for (key, value) in std::env::vars() {
        let Some(rest) = key.strip_prefix(LEGACY_PREFIX) else {
            continue;
        };
        let new_key = format!("{PREFIX}{rest}");
        if std::env::var_os(&new_key).is_none() {
            std::env::set_var(&new_key, value);
            adopted.push(key);
        }
    }
    adopted.sort();
    adopted
}

/// `./data/justtype.db`, unless only a database from before the rename is
/// there — then that one, rather than starting again with an empty one.
fn default_database_url() -> String {
    const NEW: &str = "./data/justtype.db";
    const OLD: &str = "./data/scriptorium.db";
    if !std::path::Path::new(NEW).exists() && std::path::Path::new(OLD).exists() {
        OLD.to_string()
    } else {
        NEW.to_string()
    }
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind_addr = env_or("JUSTTYPE_BIND_ADDR", "127.0.0.1:8088").parse()?;
        let jwt_secret = env_or("JUSTTYPE_JWT_SECRET", "");
        if jwt_secret.len() < 16 {
            anyhow::bail!("JUSTTYPE_JWT_SECRET must be set to at least 16 characters");
        }
        Ok(Self {
            bind_addr,
            database_url: std::env::var("JUSTTYPE_DATABASE_URL")
                .unwrap_or_else(|_| default_database_url()),
            data_dir: PathBuf::from(env_or("JUSTTYPE_DATA_DIR", "./data/works")),
            jwt_secret,
            access_ttl_minutes: env_or("JUSTTYPE_ACCESS_TTL_MINUTES", "30").parse()?,
            refresh_ttl_days: env_or("JUSTTYPE_REFRESH_TTL_DAYS", "30").parse()?,
            cors_origins: env_or("JUSTTYPE_CORS_ORIGINS", "http://localhost:5183")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            // Set to stop a script rather than a person: nobody writing real
            // feedback comes near twenty considered notes in a minute.
            comment_rate: env_or("JUSTTYPE_COMMENT_RATE", "20").parse()?,
            vote_rate: env_or("JUSTTYPE_VOTE_RATE", "120").parse()?,
            reaction_rate: env_or("JUSTTYPE_REACTION_RATE", "120").parse()?,
            second_factor_rate: env_or("JUSTTYPE_SECOND_FACTOR_RATE", "5").parse()?,
            public_url: env_or("JUSTTYPE_PUBLIC_URL", "http://localhost:5183")
                .trim_end_matches('/')
                .to_string(),
            backup_dir: std::env::var("JUSTTYPE_BACKUP_DIR")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            backup_keep: env_or("JUSTTYPE_BACKUP_KEEP", "14").parse()?,
            log_format: env_or("JUSTTYPE_LOG_FORMAT", "text"),
            cache_mb: env_or("JUSTTYPE_CACHE_MB", "64").parse()?,
            ip_rate: env_or("JUSTTYPE_IP_RATE", "600").parse()?,
            auth_ip_rate: env_or("JUSTTYPE_AUTH_IP_RATE", "30").parse()?,
            trust_proxy: env_or("JUSTTYPE_TRUST_PROXY", "false") == "true",
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .filter(|k| !k.trim().is_empty()),
            anthropic_url: env_or("JUSTTYPE_ANTHROPIC_URL", "https://api.anthropic.com")
                .trim_end_matches('/')
                .to_string(),
            summary_model: env_or("JUSTTYPE_SUMMARY_MODEL", "claude-opus-5-5"),
            assistant_model: env_or("JUSTTYPE_ASSISTANT_MODEL", "claude-opus-5-5"),
            web_dir: std::env::var("JUSTTYPE_WEB_DIR")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            metrics_token: std::env::var("JUSTTYPE_METRICS_TOKEN")
                .ok()
                .filter(|s| s.len() >= 16),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_under_the_old_name_still_count() {
        // Names no other test reads, since the environment is shared.
        std::env::set_var("SCRIPTORIUM_RENAME_TEST_OLD_ONLY", "kept");
        std::env::set_var("SCRIPTORIUM_RENAME_TEST_BOTH", "old");
        std::env::set_var("JUSTTYPE_RENAME_TEST_BOTH", "new");
        let adopted = adopt_legacy_env();
        assert_eq!(
            std::env::var("JUSTTYPE_RENAME_TEST_OLD_ONLY").unwrap(),
            "kept"
        );
        assert_eq!(std::env::var("JUSTTYPE_RENAME_TEST_BOTH").unwrap(), "new");
        assert!(adopted.contains(&"SCRIPTORIUM_RENAME_TEST_OLD_ONLY".to_string()));
        assert!(!adopted.contains(&"SCRIPTORIUM_RENAME_TEST_BOTH".to_string()));
    }
}
