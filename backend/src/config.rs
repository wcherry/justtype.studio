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
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind_addr = env_or("SCRIPTORIUM_BIND_ADDR", "127.0.0.1:8088").parse()?;
        let jwt_secret = env_or("SCRIPTORIUM_JWT_SECRET", "");
        if jwt_secret.len() < 16 {
            anyhow::bail!("SCRIPTORIUM_JWT_SECRET must be set to at least 16 characters");
        }
        Ok(Self {
            bind_addr,
            database_url: env_or("SCRIPTORIUM_DATABASE_URL", "./data/scriptorium.db"),
            data_dir: PathBuf::from(env_or("SCRIPTORIUM_DATA_DIR", "./data/works")),
            jwt_secret,
            access_ttl_minutes: env_or("SCRIPTORIUM_ACCESS_TTL_MINUTES", "30").parse()?,
            refresh_ttl_days: env_or("SCRIPTORIUM_REFRESH_TTL_DAYS", "30").parse()?,
            cors_origins: env_or("SCRIPTORIUM_CORS_ORIGINS", "http://localhost:5183")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            // Set to stop a script rather than a person: nobody writing real
            // feedback comes near twenty considered notes in a minute.
            comment_rate: env_or("SCRIPTORIUM_COMMENT_RATE", "20").parse()?,
            vote_rate: env_or("SCRIPTORIUM_VOTE_RATE", "120").parse()?,
            reaction_rate: env_or("SCRIPTORIUM_REACTION_RATE", "120").parse()?,
            second_factor_rate: env_or("SCRIPTORIUM_SECOND_FACTOR_RATE", "5").parse()?,
            public_url: env_or("SCRIPTORIUM_PUBLIC_URL", "http://localhost:5183")
                .trim_end_matches('/')
                .to_string(),
            backup_dir: std::env::var("SCRIPTORIUM_BACKUP_DIR")
                .ok()
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
            backup_keep: env_or("SCRIPTORIUM_BACKUP_KEEP", "14").parse()?,
            log_format: env_or("SCRIPTORIUM_LOG_FORMAT", "text"),
            cache_mb: env_or("SCRIPTORIUM_CACHE_MB", "64").parse()?,
            ip_rate: env_or("SCRIPTORIUM_IP_RATE", "600").parse()?,
            auth_ip_rate: env_or("SCRIPTORIUM_AUTH_IP_RATE", "30").parse()?,
            trust_proxy: env_or("SCRIPTORIUM_TRUST_PROXY", "false") == "true",
            metrics_token: std::env::var("SCRIPTORIUM_METRICS_TOKEN")
                .ok()
                .filter(|s| s.len() >= 16),
        })
    }
}
