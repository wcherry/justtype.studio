mod ai;
mod auth;
mod cache;
mod config;
mod db;
mod domain;
mod error;
mod export;
mod limit;
mod live;
mod mail;
mod metrics;
mod ops;
mod routes;
mod state;
mod storage;
mod thesaurus;
mod web;

/// The version this build reports: the release stamped in by the image build
/// (`JUSTTYPE_VERSION`, from the CI run), or the crate's own otherwise.
pub fn app_version() -> &'static str {
    match option_env!("JUSTTYPE_VERSION") {
        Some(v) if !v.is_empty() => v,
        _ => env!("CARGO_PKG_VERSION"),
    }
}

use std::sync::Arc;

use axum::http::{header, HeaderValue, Method};
use tower_http::cors::CorsLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::Config;
use crate::state::AppState;
use crate::storage::FileStore;

/// A handler that panicked still answers in the envelope every client
/// understands, and the panic is logged inside the request's span.
fn panicked(err: Box<dyn std::any::Any + Send + 'static>) -> axum::response::Response {
    let detail = err
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "unknown".into());
    tracing::error!(panic = %detail, "handler panicked");
    axum::response::IntoResponse::into_response(error::ApiError::internal(
        "something went wrong on our side",
    ))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // `--openapi` prints the document and exits, so generating it needs no
    // database, no data directory and no port.
    if std::env::args().any(|a| a == "--openapi") {
        println!("{}", routes::openapi_document().to_pretty_json()?);
        return Ok(());
    }

    dotenvy::dotenv().ok();
    let legacy_settings = config::adopt_legacy_env();
    // `--tune-argon2 [ms]` times password hashing on this machine and exits.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--tune-argon2") {
        let target = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(250);
        auth::password::tune(target);
        return Ok(());
    }
    let config = Config::from_env()?;
    auth::password::configure(auth::password::Cost::from_env()?);
    routes::share::configure(&config.jwt_secret);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "justtype_backend=debug,tower_http=info,axum=info".into());
    // JSON in production, so a log shipper can index on request_id; text for
    // a person reading a terminal. On stderr: stdout is the answer of the
    // maintenance commands (`--backup` prints where it put the backup), and a
    // script reading it must not get a log line instead.
    if config.log_format == "json" {
        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::io::stderr)
                    .json()
                    .with_current_span(true)
                    .with_span_list(false),
            )
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .init();
    }
    if !legacy_settings.is_empty() {
        tracing::warn!(
            "read settings under their old names; rename SCRIPTORIUM_ to JUSTTYPE_ (`cargo setup` does it for .env): {}",
            legacy_settings.join(", ")
        );
    }
    if config.database_url.ends_with("scriptorium.db") {
        tracing::warn!(
            "using {}, the database from before the rename; it works as it is, or rename it to justtype.db",
            config.database_url
        );
    }

    // Maintenance runs as the same binary, against the same configuration,
    // and exits: `--backup` takes one, `--restore-drill <dir>` proves one.
    if args.iter().any(|a| a == "--backup") {
        let into = config
            .backup_dir
            .clone()
            .ok_or_else(|| anyhow::anyhow!("set JUSTTYPE_BACKUP_DIR to say where backups go"))?;
        let taken = ops::backup(
            &config.database_url,
            &config.data_dir,
            &into,
            config.backup_keep,
        )?;
        println!("{}", taken.display());
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--restore-drill") {
        let target = match args.get(i + 1) {
            Some(path) => std::path::PathBuf::from(path),
            None => config
                .backup_dir
                .as_deref()
                .and_then(ops::latest_backup)
                .ok_or_else(|| anyhow::anyhow!("no backup to drill"))?,
        };
        let report = ops::restore_drill(&target)?;
        for check in &report.checks {
            println!(
                "  {} {:<18} {}",
                if check.ok { "ok  " } else { "FAIL" },
                check.name,
                check.detail
            );
        }
        if !report.passed {
            anyhow::bail!("the restore drill failed for {}", target.display());
        }
        println!("restored and checked: {}", target.display());
        return Ok(());
    }
    let pool = db::build_pool(&config.database_url)?;
    db::run_migrations(&pool)?;

    // Built-in templates are refreshed on every boot so editing the template
    // file is all it takes to ship a new one.
    {
        let mut conn = pool.get()?;
        domain::templates::seed(&mut conn, &domain::now_iso())?;
        // Where a deleted account's notes are credited, so threads keep their
        // shape.
        routes::privacy::seed_former_member(&mut conn, &domain::now_iso())?;
    }

    // Samples how long a writer waits for SQLite's write lock.
    metrics::spawn_lock_probe(pool.clone());

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
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            ops::REQUEST_ID_HEADER.clone(),
            axum::http::HeaderName::from_static("x-share-key"),
        ])
        // So a browser on another origin can read the id it should quote.
        .expose_headers([ops::REQUEST_ID_HEADER.clone()]);

    let bind_addr = config.bind_addr;
    let web_dir = config.web_dir.clone();
    let cache_bytes = config.cache_mb * 1024 * 1024;
    let mailer = mail::Mailer::from_env(&config.data_dir)?;
    let state = AppState {
        mailer,
        started_at: std::time::Instant::now(),
        config: Arc::new(config),
        pool,
        store,
        limiter: Arc::new(limit::RateLimiter::new()),
        scene_cache: Arc::new(cache::SceneCache::new(cache_bytes)),
        hub: Arc::new(live::Hub::default()),
        login_guard: Arc::new(limit::LoginGuard::default()),
    };

    // Outermost first in effect: the request id wraps everything, so even a
    // panic's 500 and the CORS preflight carry one.
    let mut app = routes::router(state.clone());
    match web_dir {
        Some(dir) if web::present(&dir) => {
            tracing::info!("serving the web app from {}", dir.display());
            app = web::mount(app, dir);
        }
        Some(dir) => tracing::warn!(
            "JUSTTYPE_WEB_DIR is {}, but there is no index.html there; serving the API only",
            dir.display()
        ),
        None => {}
    }
    let app = app
        .layer(tower_http::catch_panic::CatchPanicLayer::custom(panicked))
        // Before CORS and the handlers, so a flood is turned away cheaply.
        .layer(axum::middleware::from_fn_with_state(
            state,
            ops::per_address,
        ))
        .layer(cors)
        .layer(axum::middleware::from_fn(ops::request_id));

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    tracing::info!("justtype listening on http://{bind_addr}");
    // With the peer address attached, so a session can say where it signed in
    // from.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
