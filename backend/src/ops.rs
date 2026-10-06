//! Running it: a request id through every log line and error, a status check
//! that tests the things that actually fail, and backups that are proved by
//! restoring them.
use std::path::{Path, PathBuf};
use std::time::Instant;

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::Instrument;

pub static REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

tokio::task_local! {
    /// The id of the request being served, for error envelopes. Set by
    /// `request_id` around every handler.
    pub static REQUEST_ID: String;
}

/// The id to quote back: the client's own if it sent a sensible one, so a
/// support conversation can start from what the browser saw, otherwise ours.
fn pick_id(incoming: Option<&HeaderValue>) -> String {
    incoming
        .and_then(|v| v.to_str().ok())
        .filter(|v| {
            (8..=64).contains(&v.len())
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string())
}

/// Gives every request an id, puts it on the span every log line inherits,
/// on the response, and within reach of the error envelope.
pub async fn request_id(mut request: Request, next: Next) -> Response {
    let id = pick_id(request.headers().get(&REQUEST_ID_HEADER));
    if let Ok(value) = HeaderValue::from_str(&id) {
        request
            .headers_mut()
            .insert(REQUEST_ID_HEADER.clone(), value);
    }
    let span = tracing::info_span!(
        "request",
        request_id = %id,
        method = %request.method(),
        path = %request.uri().path(),
    );
    let started = Instant::now();
    let writing = !matches!(
        *request.method(),
        axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
    ) && request.uri().path().starts_with("/api/");
    let keys = crate::routes::share::keys_from(request.headers());
    let mut response = REQUEST_ID
        .scope(
            id.clone(),
            crate::metrics::WRITING.scope(
                writing,
                crate::routes::share::PRESENTED
                    .scope(keys, next.run(request).instrument(span.clone())),
            ),
        )
        .await;
    if writing {
        crate::metrics::global().record_write_request(started.elapsed());
    }
    let status = response.status();
    span.in_scope(|| {
        let ms = started.elapsed().as_millis() as u64;
        if status.is_server_error() {
            tracing::error!(status = status.as_u16(), ms, "failed");
        } else {
            tracing::info!(status = status.as_u16(), ms, "served");
        }
    });
    if let Ok(value) = HeaderValue::from_str(&id) {
        response
            .headers_mut()
            .insert(REQUEST_ID_HEADER.clone(), value);
    }
    response
}

/// Where a request came from: the peer, or behind a trusted proxy the last
/// address the proxy appended. Never the first X-Forwarded-For entry, which
/// is whatever the client wrote.
pub fn client_ip(request: &Request, trust_proxy: bool) -> Option<String> {
    if trust_proxy {
        if let Some(last) = request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return Some(last.to_string());
        }
    }
    request
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|info| info.0.ip().to_string())
}

/// Per-address rate limits: everything, and the sign-in routes much more
/// tightly. They stop one machine hammering; `limit::LoginGuard` stops many
/// machines guessing at one account slowly.
pub async fn per_address(
    axum::extract::State(state): axum::extract::State<crate::state::AppState>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    // Health checks are for monitors, and a monitor should never be told no.
    // The web app's own files are not the API: a page load fetches several,
    // and they should not spend a reader's budget for real requests.
    if path == "/api/health" || path == "/api/status" || !path.starts_with("/api") {
        return next.run(request).await;
    }
    let Some(ip) = client_ip(&request, state.config.trust_proxy) else {
        return next.run(request).await;
    };
    let limiter = &state.limiter;
    let mut verdict = limiter.check(&ip, crate::limit::Action::Requests, state.config.ip_rate);
    if verdict.is_ok() && path.starts_with("/api/auth/") {
        verdict = limiter.check(
            &ip,
            crate::limit::Action::AuthRequests,
            state.config.auth_ip_rate,
        );
    }
    match verdict {
        Ok(()) => next.run(request).await,
        Err(refused) => axum::response::IntoResponse::into_response(refused),
    }
}

/// The current request's id, when there is one.
pub fn current_request_id() -> Option<String> {
    REQUEST_ID.try_with(|id| id.clone()).ok()
}

// --- backups -------------------------------------------------------------

/// What a backup says about itself, so a drill can check it is whole.
#[derive(Debug, Serialize, Deserialize, Clone, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub created_at: String,
    pub app_version: String,
    pub database_sha256: String,
    /// Row counts for the tables a writer would miss.
    pub counts: std::collections::BTreeMap<String, i64>,
    /// Every manuscript file, relative to the works folder, with its digest.
    pub files: std::collections::BTreeMap<String, String>,
}

/// What a restore drill found.
#[derive(Debug, Serialize, Deserialize, Clone, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DrillReport {
    pub backup: String,
    pub ran_at: String,
    pub passed: bool,
    pub checks: Vec<Check>,
}

#[derive(Debug, Serialize, Deserialize, Clone, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

const COUNTED: &[&str] = &[
    "users",
    "works",
    "chapters",
    "scenes",
    "versions",
    "version_entries",
    "publications",
    "comments",
];

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}

fn counts(conn: &mut SqliteConnection) -> anyhow::Result<std::collections::BTreeMap<String, i64>> {
    #[derive(QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    let mut out = std::collections::BTreeMap::new();
    for table in COUNTED {
        // The names are ours, from the list above; nothing here comes from a
        // request.
        let row: Count =
            diesel::sql_query(format!("SELECT COUNT(*) AS n FROM {table}")).get_result(conn)?;
        out.insert(table.to_string(), row.n);
    }
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            walk(root, &path, out)?;
        } else {
            out.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
        }
    }
    Ok(())
}

/// Takes one backup into a new timestamped folder under `into`, and prunes
/// all but the newest `keep`.
///
/// The database comes first, with `VACUUM INTO`, which is a consistent copy
/// even while the server is writing. The manuscript files follow. Scene files
/// are only ever replaced whole (write, then rename) and a snapshot's files
/// never change, so each file is consistent; the one gap is a scene deleted
/// in the seconds between the two steps, which the drill would report.
pub fn backup(
    database_url: &str,
    data_dir: &Path,
    into: &Path,
    keep: usize,
) -> anyhow::Result<PathBuf> {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let dest = into.join(&stamp);
    std::fs::create_dir_all(&dest)?;

    let db_copy = dest.join("scriptorium.db");
    let mut conn = SqliteConnection::establish(database_url)?;
    diesel::sql_query(format!(
        "VACUUM INTO '{}'",
        db_copy.to_string_lossy().replace('\'', "''")
    ))
    .execute(&mut conn)?;
    let mut copy = SqliteConnection::establish(&db_copy.to_string_lossy())?;
    let counts = counts(&mut copy)?;
    drop(copy);

    let mut files = std::collections::BTreeMap::new();
    let mut found = Vec::new();
    walk(data_dir, data_dir, &mut found)?;
    for relative in found {
        let from = data_dir.join(&relative);
        let to = dest.join("works").join(&relative);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&from, &to)?;
        files.insert(
            relative.to_string_lossy().replace('\\', "/"),
            sha256_file(&to)?,
        );
    }

    let manifest = Manifest {
        created_at: crate::domain::now_iso(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        database_sha256: sha256_file(&db_copy)?,
        counts,
        files,
    };
    std::fs::write(
        dest.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    // Oldest first by name, which is the timestamp.
    let mut all: Vec<PathBuf> = std::fs::read_dir(into)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("manifest.json").exists())
        .collect();
    all.sort();
    if all.len() > keep {
        for old in &all[..all.len() - keep] {
            std::fs::remove_dir_all(old)?;
        }
    }
    Ok(dest)
}

/// Restores a backup somewhere disposable and checks it would actually serve:
/// the database opens, passes SQLite's own integrity check and takes this
/// build's migrations; the counts and every file match what the backup says;
/// and every scene and snapshot the database knows about has its file.
///
/// Writes the report next to the backup, where the status check reads it.
pub fn restore_drill(backup_dir: &Path) -> anyhow::Result<DrillReport> {
    let mut checks = Vec::new();
    let mut check = |name: &str, ok: bool, detail: String| {
        checks.push(Check {
            name: name.into(),
            ok,
            detail,
        })
    };

    let manifest: Manifest =
        serde_json::from_str(&std::fs::read_to_string(backup_dir.join("manifest.json"))?)?;
    let scratch = std::env::temp_dir().join(format!("scriptorium-drill-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&scratch)?;
    let restored_db = scratch.join("scriptorium.db");
    std::fs::copy(backup_dir.join("scriptorium.db"), &restored_db)?;

    let digest = sha256_file(&restored_db)?;
    check(
        "database file",
        digest == manifest.database_sha256,
        "matches the digest taken when it was backed up".into(),
    );

    let pool = crate::db::build_pool_of(&restored_db.to_string_lossy(), 1)?;
    let mut conn = pool.get()?;
    #[derive(QueryableByName)]
    struct Integrity {
        #[diesel(sql_type = diesel::sql_types::Text)]
        integrity_check: String,
    }
    let integrity: Vec<Integrity> = diesel::sql_query("PRAGMA integrity_check").load(&mut conn)?;
    let healthy = integrity.len() == 1 && integrity[0].integrity_check == "ok";
    check(
        "integrity",
        healthy,
        integrity
            .iter()
            .map(|r| r.integrity_check.as_str())
            .collect::<Vec<_>>()
            .join("; "),
    );
    drop(conn);

    match crate::db::run_migrations(&pool) {
        Ok(()) => check("migrations", true, "this build's schema applies".into()),
        Err(e) => check("migrations", false, e.to_string()),
    }

    let mut conn = pool.get()?;
    let now = counts(&mut conn)?;
    let mismatched: Vec<String> = manifest
        .counts
        .iter()
        .filter(|(table, n)| now.get(*table) != Some(n))
        .map(|(table, n)| {
            format!(
                "{table}: {n} backed up, {} restored",
                now.get(table).copied().unwrap_or(-1)
            )
        })
        .collect();
    check(
        "row counts",
        mismatched.is_empty(),
        if mismatched.is_empty() {
            now.iter()
                .map(|(t, n)| format!("{t} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            mismatched.join("; ")
        },
    );

    let works = backup_dir.join("works");
    let mut bad = Vec::new();
    for (relative, digest) in &manifest.files {
        match sha256_file(&works.join(relative)) {
            Ok(found) if &found == digest => {}
            Ok(_) => bad.push(format!("{relative} differs")),
            Err(_) => bad.push(format!("{relative} missing")),
        }
    }
    check(
        "manuscript files",
        bad.is_empty(),
        if bad.is_empty() {
            format!("{} files, every one as backed up", manifest.files.len())
        } else {
            bad.into_iter().take(10).collect::<Vec<_>>().join("; ")
        },
    );

    // The test that matters to a writer: can every scene be opened?
    use crate::db::schema::{scenes, version_entries, versions};
    let working: Vec<(String, String)> = scenes::table
        .select((scenes::work_id, scenes::id))
        .load(&mut conn)?;
    let frozen: Vec<(String, String, String)> = version_entries::table
        .inner_join(versions::table)
        .select((versions::work_id, versions::id, version_entries::scene_id))
        .load(&mut conn)?;
    let mut missing: Vec<String> = Vec::new();
    for (work, scene) in &working {
        if !works
            .join(work)
            .join("working")
            .join(format!("{scene}.json"))
            .exists()
        {
            missing.push(format!("draft of scene {scene}"));
        }
    }
    for (work, version, scene) in &frozen {
        let path = works
            .join(work)
            .join("versions")
            .join(version)
            .join("scenes")
            .join(format!("{scene}.json"));
        if !path.exists() {
            missing.push(format!("scene {scene} in snapshot {version}"));
        }
    }
    check(
        "every scene opens",
        missing.is_empty(),
        if missing.is_empty() {
            format!(
                "{} drafts and {} snapshot scenes have their files",
                working.len(),
                frozen.len()
            )
        } else {
            format!(
                "{} missing: {}",
                missing.len(),
                missing.into_iter().take(5).collect::<Vec<_>>().join("; ")
            )
        },
    );
    drop(conn);
    drop(pool);
    std::fs::remove_dir_all(&scratch).ok();

    let report = DrillReport {
        backup: backup_dir.to_string_lossy().into(),
        ran_at: crate::domain::now_iso(),
        passed: checks.iter().all(|c| c.ok),
        checks,
    };
    std::fs::write(
        backup_dir.join("drill.json"),
        serde_json::to_string_pretty(&report)?,
    )?;
    Ok(report)
}

/// The newest backup under a folder, if any.
pub fn latest_backup(dir: &Path) -> Option<PathBuf> {
    let mut all: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("manifest.json").exists())
        .collect();
    all.sort();
    all.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behind_a_proxy_the_address_is_the_one_it_added_not_the_one_offered() {
        let request = Request::builder()
            .header("x-forwarded-for", "6.6.6.6, 203.0.113.9")
            .body(axum::body::Body::empty())
            .unwrap();
        assert_eq!(client_ip(&request, true).as_deref(), Some("203.0.113.9"));
        // Not trusted: the header is ignored entirely.
        assert_eq!(client_ip(&request, false), None);
    }

    #[test]
    fn a_sensible_client_id_is_kept_and_anything_else_replaced() {
        let ours = HeaderValue::from_static("web-1a2b3c4d");
        assert_eq!(pick_id(Some(&ours)), "web-1a2b3c4d");
        for junk in ["short", "has spaces in it", "<script>alert(1)</script>"] {
            let value = HeaderValue::from_str(junk).unwrap();
            let picked = pick_id(Some(&value));
            assert_ne!(picked, junk);
            assert_eq!(picked.len(), 32);
        }
        assert_eq!(pick_id(None).len(), 32);
    }

    /// The whole loop on a real database and data folder: back up, drill,
    /// then damage the backup and watch the drill notice.
    #[test]
    fn a_backup_restores_and_a_damaged_one_is_caught() {
        let root = std::env::temp_dir().join(format!("scriptorium-ops-{}", uuid::Uuid::new_v4()));
        let data = root.join("works");
        let db = root.join("live.db");
        std::fs::create_dir_all(data.join("w1/working")).unwrap();
        let pool = crate::db::build_pool(db.to_str().unwrap()).unwrap();
        crate::db::run_migrations(&pool).unwrap();
        {
            let mut conn = pool.get().unwrap();
            let now = crate::domain::now_iso();
            diesel::sql_query(format!(
                "INSERT INTO users (id, email, display_name, password_hash, created_at, updated_at) \
                 VALUES ('u1', 'a@example.com', 'A', '!', '{now}', '{now}');"
            ))
            .execute(&mut conn)
            .unwrap();
            diesel::sql_query(format!(
                "INSERT INTO works (id, owner_id, title, kind, status, word_count, created_at, updated_at) \
                 VALUES ('w1', 'u1', 'T', 'novel', 'draft', 0, '{now}', '{now}');"
            ))
            .execute(&mut conn)
            .unwrap();
            diesel::sql_query(format!(
                "INSERT INTO chapters (id, work_id, title, position, created_at, updated_at) \
                 VALUES ('c1', 'w1', 'One', 0, '{now}', '{now}');"
            ))
            .execute(&mut conn)
            .unwrap();
            diesel::sql_query(format!(
                "INSERT INTO scenes (id, work_id, chapter_id, title, status, position, word_count, content_hash, created_at, updated_at) \
                 VALUES ('s1', 'w1', 'c1', 'S', 'drafting', 0, 0, 'h', '{now}', '{now}');"
            ))
            .execute(&mut conn)
            .unwrap();
        }
        std::fs::write(data.join("w1/working/s1.json"), b"{\"type\":\"doc\"}").unwrap();

        let backups = root.join("backups");
        let taken = backup(db.to_str().unwrap(), &data, &backups, 3).unwrap();
        let report = restore_drill(&taken).unwrap();
        assert!(report.passed, "{report:#?}");
        assert_eq!(latest_backup(&backups).unwrap(), taken);

        // A manuscript file lost from the backup is exactly what a drill is for.
        std::fs::remove_file(taken.join("works/w1/working/s1.json")).unwrap();
        let report = restore_drill(&taken).unwrap();
        assert!(!report.passed);
        let failed: Vec<&str> = report
            .checks
            .iter()
            .filter(|c| !c.ok)
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(failed, ["manuscript files", "every scene opens"]);

        std::fs::remove_dir_all(&root).ok();
    }
}
