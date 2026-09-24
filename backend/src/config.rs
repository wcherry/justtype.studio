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
        })
    }
}
