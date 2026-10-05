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

    let healthy = checks.iter().all(|c| c.ok);
    let report = StatusReport {
        status: if healthy { "ok" } else { "degraded" }.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        uptime_seconds: state.started_at.elapsed().as_secs(),
        checks,
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
