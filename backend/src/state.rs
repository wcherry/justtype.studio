use std::sync::Arc;

use crate::config::Config;
use crate::db::Pool;
use crate::limit::RateLimiter;
use crate::mail::Mailer;
use crate::storage::FileStore;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: Pool,
    pub store: FileStore,
    /// In process and in memory — see `limit`. Shared by clone, so every
    /// handler counts into the same buckets.
    pub limiter: Arc<RateLimiter>,
    pub mailer: Mailer,
    pub started_at: std::time::Instant,
    /// Snapshot scene bodies — see `cache`.
    pub scene_cache: Arc<crate::cache::SceneCache>,
    /// The live channel's rooms — see `live`.
    pub hub: Arc<crate::live::Hub>,
    /// Failed sign-ins per account — see `limit::LoginGuard`.
    pub login_guard: Arc<crate::limit::LoginGuard>,
}
