//! Whether the service is fit to serve, for an uptime checker and for the
//! status page. `/api/health` says the process answers; this says whether a
//! writer's next save would land, and whether last night's backup could be
//! restored.
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use diesel::prelude::*;
use diesel_migrations::MigrationHarness;
use serde::Serialize;

use crate::db::blocking;
use crate::ops::{self, Check, DrillReport, Manifest};
use crate::state::AppState;

/// A p95 lock wait at or above this means writes are queueing behind each
/// other, which the expected load should never do.
const LOCK_WAIT_LIMIT_MS: f64 = 250.0;

/// Prometheus text for a dashboard to scrape. Exists only when
/// `SCRIPTORIUM_METRICS_TOKEN` is set, and wants it as a bearer token.
#[utoipa::path(
    get,
    path = "/api/metrics",
    tag = "health",
    responses(
        (status = 200, description = "Prometheus text", content_type = "text/plain"),
        (status = 404, description = "Metrics are not turned on"),
        (status = 401, description = "Wrong or missing token"),
    )
)]
pub async fn metrics(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Result<String, StatusCode> {
    let Some(token) = &state.config.metrics_token else {
        return Err(StatusCode::NOT_FOUND);
    };
    let presented = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    // Compared by digest so the comparison takes the same time however much
    // of a guess is right.
    use sha2::{Digest, Sha256};
    if Sha256::digest(presented.as_bytes()) != Sha256::digest(token.as_bytes()) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(crate::metrics::prometheus())
}

/// A backup older than this means last night's did not happen.
const BACKUP_STALE_HOURS: i64 = 26;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusReport {
    /// `ok`, or `degraded` when any check fails.
    pub status: String,
    pub version: String,
    pub uptime_seconds: u64,
    pub checks: Vec<Check>,
    /// The last minute and the last fifteen.
    pub write_path: Vec<crate::metrics::WindowSummary>,
}

#[utoipa::path(
    get,
    path = "/api/status",
    tag = "health",
    responses(
        (status = 200, description = "Every check passed", body = StatusReport),
        (status = 503, description = "Something needs looking at", body = StatusReport),
    )
)]
pub async fn status(State(state): State<AppState>) -> (StatusCode, Json<StatusReport>) {
    let mut checks = Vec::new();

    let database = blocking(&state.pool, |conn| {
        diesel::sql_query("SELECT 1").execute(conn)?;
        let pending = conn
            .pending_migrations(crate::db::MIGRATIONS)
            .map_err(|e| crate::error::ApiError::internal(e.to_string()))?
            .len();
        Ok(pending)
    })
    .await;
    checks.push(match database {
        Ok(0) => check("database", true, "answers, and its schema is current"),
        Ok(n) => check("database", false, &format!("{n} migration(s) not applied")),
        Err(e) => check("database", false, &e.to_string()),
    });

    // A save is a write and a rename in this folder; prove both still work.
    let probe = state
        .config
        .data_dir
        .join(format!(".status-{}", uuid::Uuid::new_v4().simple()));
    let storage = async {
        tokio::fs::write(&probe, b"ok").await?;
        let moved = probe.with_extension("moved");
        tokio::fs::rename(&probe, &moved).await?;
        tokio::fs::remove_file(&moved).await
    }
    .await;
    checks.push(match storage {
        Ok(()) => check("manuscripts", true, "the folder takes a write and a rename"),
        Err(e) => check("manuscripts", false, &format!("cannot save a scene: {e}")),
    });

    checks.push(match &state.config.backup_dir {
        None => check(
            "backups",
            false,
            "SCRIPTORIUM_BACKUP_DIR is not set, so nothing is being kept",
        ),
        Some(dir) => backup_check(dir),
    });

    checks.push(check(
        "mail",
        true,
        match &state.mailer {
            crate::mail::Mailer::Outbox(_) => "writing to the outbox file; nothing is sent",
            crate::mail::Mailer::Smtp { .. } => "sending through SMTP",
        },
    ));

    // The architecture assumes a few writes a second against one file. If a
    // writer is routinely waiting a second for the lock, that assumption is
    // what has failed, and it should say so before anyone notices it.
    let recent = crate::metrics::global().summary(std::time::Duration::from_secs(15 * 60));
    checks.push(if recent.lock_wait_ms.p95 >= LOCK_WAIT_LIMIT_MS {
        check(
            "write path",
            false,
            &format!(
                "writers wait {}ms for the lock at p95 — SQLite is the bottleneck now",
                recent.lock_wait_ms.p95
            ),
        )
    } else {
        check(
            "write path",
            true,
            &format!(
                "{} writes/s, {}ms at p95, lock wait {}ms at p95 ({} samples)",
                recent.db_writes_per_second,
                recent.write_ms.p95,
                recent.lock_wait_ms.p95,
                recent.lock_samples
            ),
        )
    });

    let healthy = checks.iter().all(|c| c.ok);
    let report = StatusReport {
        status: if healthy { "ok" } else { "degraded" }.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        uptime_seconds: state.started_at.elapsed().as_secs(),
        checks,
        write_path: vec![
            crate::metrics::global().summary(std::time::Duration::from_secs(60)),
            recent,
        ],
    };
    let code = if healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, Json(report))
}

fn check(name: &str, ok: bool, detail: &str) -> Check {
    Check {
        name: name.into(),
        ok,
        detail: detail.into(),
    }
}

/// The newest backup must be recent, and its restore drill must have passed:
/// a backup nobody has restored is a hope, not a backup.
fn backup_check(dir: &std::path::Path) -> Check {
    let Some(latest) = ops::latest_backup(dir) else {
        return check("backups", false, "none taken yet");
    };
    let manifest: Option<Manifest> = std::fs::read_to_string(latest.join("manifest.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    let Some(manifest) = manifest else {
        return check(
            "backups",
            false,
            "the newest backup has no readable manifest",
        );
    };
    let age_hours = chrono::DateTime::parse_from_rfc3339(&manifest.created_at)
        .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_hours())
        .unwrap_or(i64::MAX);
    if age_hours > BACKUP_STALE_HOURS {
        return check(
            "backups",
            false,
            &format!("the newest is {age_hours} hours old"),
        );
    }
    let drill: Option<DrillReport> = std::fs::read_to_string(latest.join("drill.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok());
    match drill {
        None => check(
            "backups",
            false,
            &format!("taken {age_hours}h ago, but never restored to prove it"),
        ),
        Some(report) if !report.passed => {
            let failed: Vec<String> = report
                .checks
                .iter()
                .filter(|c| !c.ok)
                .map(|c| c.name.clone())
                .collect();
            check(
                "backups",
                false,
                &format!("the restore drill failed: {}", failed.join(", ")),
            )
        }
        Some(_) => check(
            "backups",
            true,
            &format!(
                "taken {age_hours}h ago, restored and checked: {} files",
                manifest.files.len()
            ),
        ),
    }
}
