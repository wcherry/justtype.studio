use std::sync::Arc;

use crate::config::Config;
use crate::db::Pool;
use crate::storage::FileStore;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pool: Pool,
    pub store: FileStore,
}
