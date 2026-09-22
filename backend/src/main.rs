mod auth;
mod config;
mod db;
mod domain;
mod error;
mod routes;
mod state;
mod storage;

use std::sync::Arc;

use axum::http::{header, HeaderValue, Method};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::Config;
use crate::state::AppState;
use crate::storage::FileStore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "scriptorium_backend=debug,tower_http=info,axum=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::from_env()?;
    let pool = db::build_pool(&config.database_url)?;
    db::run_migrations(&pool)?;

    // Built-in templates are refreshed on every boot so editing the template
    // file is all it takes to ship a new one.
    {
        let mut conn = pool.get()?;
        domain::templates::seed(&mut conn, &domain::now_iso())?;
    }

    let store = FileStore::new(&config.data_dir);
    store.init()?;

    let origins: Vec<HeaderValue> = config
        .cors_origins
        .iter()
        .filter_map(|o| o.parse::<HeaderValue>().ok())
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    let bind_addr = config.bind_addr;
    let state = AppState {
        config: Arc::new(config),
        pool,
        store,
    };

    let app = routes::router(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!("scriptorium listening on http://{bind_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
