//! Everything around an export that is not the export itself: profiles you
//! can save and reuse, the settings last used for each book, "Export again",
//! a history of what was sent, all formats at once, and a preview.
use std::io::{Cursor, Write};

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::{ExportHistoryRow, ExportProfile, NewExportProfile};
use crate::db::schema::{export_history, export_last_options, export_profiles, versions};
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::export::assemble::{self, Resolved};
use crate::export::options::{
    CommentsMode, ContentOptions, EpubOptions, ExportOptions, Format, HtmlMode, HtmlOptions,
    StyleOptions, StylePreset,
};
use crate::export::pipeline::{Issue, Severity};
use crate::state::AppState;

use super::export::{build_file, checked, remember, resolve_draft, resolve_version, respond};
use super::support;

const MAX_PROFILES: i64 = 50;
const NAME_LIMIT: usize = 100;
const HISTORY_LIMIT: i64 = 200;

// ── Profiles ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    /// `builtin:…` for the ones that ship with the app.
    pub id: String,
    pub name: String,
    pub builtin: bool,
    pub description: Option<String>,
    pub options: ExportOptions,
    /// What had to change to make a saved profile work today: a font that was
    /// withdrawn, a setting that was renamed.
    pub notes: Vec<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfilesResponse {
    pub builtin: Vec<ProfileView>,
    pub custom: Vec<ProfileView>,
}

/// The profiles that ship with the app. Read-only, but they can be copied.
///
/// None of them includes the writer's comments except the one that is about
/// them: a comment saved into a submission profile is how private notes end up
/// in front of an agent.
pub fn builtin_profiles() -> Vec<(&'static str, &'static str, &'static str, ExportOptions)> {
    let book = || StyleOptions {
        preset: StylePreset::Book,
        ..Default::default()
    };
    vec![
        (
            "standard-manuscript",
            "Standard manuscript",
            "A submission: Courier, double spaced, with your contact details and a word count.",
            ExportOptions::default(),
        ),
        (
            "reading-copy",
            "Reading copy (PDF)",
            "A PDF to read or print, with a contents page.",
            ExportOptions {
                format: Format::Pdf,
                style: book(),
                content: ContentOptions {
                    toc: Some(true),
                    ..Default::default()
                },
                ..Default::default()
            },
        ),
        (
            "epub",
            "EPUB for e-readers",
            "One file per chapter, with a cover and a contents list.",
            ExportOptions {
                format: Format::Epub,
                style: book(),
                content: ContentOptions {
                    toc: Some(true),
                    ..Default::default()
                },
                epub: Some(EpubOptions::default()),
                ..Default::default()
            },
        ),
        (
            "web",
            "Web publication",
            "A zip of web pages, one per chapter, that opens offline.",
            ExportOptions {
                format: Format::Html,
                style: book(),
                content: ContentOptions {
                    toc: Some(true),
                    ..Default::default()
                },
                html: Some(HtmlOptions {
                    mode: HtmlMode::Package,
                }),
                ..Default::default()
            },
        ),
        (
            "clean-html",
            "Clean HTML",
            "Plain markup with no styles, to paste into a website or convert.",
            ExportOptions {
                format: Format::Html,
                style: book(),
                content: ContentOptions {
                    front_matter: false,
                    ..Default::default()
                },
                html: Some(HtmlOptions {
                    mode: HtmlMode::Clean,
                }),
                ..Default::default()
            },
        ),
        (
            "working-copy",
            "Working copy with comments",
            "Word footnotes carrying your own comments, to work from or send an editor.",
            ExportOptions {
                format: Format::Docx,
                style: book(),
                content: ContentOptions {
                    comments: CommentsMode::Footnotes,
                    ..Default::default()
                },
                ..Default::default()
            },
        ),
    ]
}

fn builtin_views() -> Vec<ProfileView> {
    builtin_profiles()
        .into_iter()
        .map(|(id, name, description, mut options)| {
            options.profile = Some(name.to_string());
            ProfileView {
                id: format!("builtin:{id}"),
                name: name.into(),
                builtin: true,
                description: Some(description.into()),
                options,
                notes: Vec::new(),
                updated_at: None,
            }
        })
        .collect()
}

fn view_of(row: ExportProfile) -> ProfileView {
    let raw: serde_json::Value = serde_json::from_str(&row.options).unwrap_or_default();
    let (mut options, notes) = ExportOptions::repaired(raw);
    options.profile = Some(row.name.clone());
    ProfileView {
        id: row.id,
        name: row.name,
        builtin: false,
        description: None,
        options,
        notes,
        updated_at: Some(row.updated_at),
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveProfileRequest {
    pub name: String,
    pub options: ExportOptions,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProfileRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub options: Option<ExportOptions>,
}

fn clean_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest(
            "give the profile a name so you can find it again".into(),
        ));
    }
    if name.chars().count() > NAME_LIMIT {
        return Err(ApiError::BadRequest(format!(
            "a profile name can be at most {NAME_LIMIT} characters"
        )));
    }
    Ok(name.to_string())
}

#[utoipa::path(
    get,
    path = "/api/export/profiles",
    tag = "export",
    responses(
        (status = 200, description = "Built-in and saved profiles", body = ProfilesResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_profiles(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<ProfilesResponse>> {
    let rows = blocking(&state.pool, move |conn| {
        Ok(export_profiles::table
            .filter(export_profiles::user_id.eq(&user.id))
            .order((export_profiles::name.asc(), export_profiles::created_at.asc()))
            .select(ExportProfile::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(Json(ProfilesResponse {
        builtin: builtin_views(),
        custom: rows.into_iter().map(view_of).collect(),
    }))
}

#[utoipa::path(
    post,
    path = "/api/export/profiles",
    tag = "export",
    request_body = SaveProfileRequest,
    responses(
        (status = 200, description = "Saved", body = ProfileView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create_profile(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<SaveProfileRequest>,
) -> ApiResult<Json<ProfileView>> {
    let name = clean_name(&body.name)?;
    let options = checked(body.options)?.as_profile();
    let json = serde_json::to_string(&options)?;
    let row = blocking(&state.pool, move |conn| {
        let held: i64 = export_profiles::table
            .filter(export_profiles::user_id.eq(&user.id))
            .count()
            .get_result(conn)?;
        if held >= MAX_PROFILES {
            return Err(ApiError::BadRequest(format!(
                "you can keep {MAX_PROFILES} profiles; delete one you no longer use"
            )));
        }
        let now = now_iso();
        let id = new_id();
        diesel::insert_into(export_profiles::table)
            .values(&NewExportProfile {
                id: id.clone(),
                user_id: user.id.clone(),
                name,
                options: json,
                created_at: now.clone(),
                updated_at: now,
            })
            .execute(conn)?;
        Ok(export_profiles::table
            .find(&id)
            .select(ExportProfile::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(view_of(row)))
}

#[utoipa::path(
    patch,
    path = "/api/export/profiles/{profile_id}",
    tag = "export",
    params(("profile_id" = String, Path)),
    request_body = UpdateProfileRequest,
    responses(
        (status = 200, description = "Saved", body = ProfileView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update_profile(
    State(state): State<AppState>,
    user: AuthUser,
    Path(profile_id): Path<String>,
    Json(body): Json<UpdateProfileRequest>,
) -> ApiResult<Json<ProfileView>> {
    if profile_id.starts_with("builtin:") {
        return Err(ApiError::BadRequest(
            "the built-in profiles cannot be changed. Duplicate it, then change the copy.".into(),
        ));
    }
    let name = body.name.as_deref().map(clean_name).transpose()?;
    let options = match body.options {
        Some(o) => Some(serde_json::to_string(&checked(o)?.as_profile())?),
        None => None,
    };
    let row = blocking(&state.pool, move |conn| {
        let owned: Option<String> = export_profiles::table
            .filter(export_profiles::id.eq(&profile_id))
            .filter(export_profiles::user_id.eq(&user.id))
            .select(export_profiles::id)
            .first(conn)
            .optional()?;
        if owned.is_none() {
            return Err(ApiError::NotFound("no such profile".into()));
        }
        if let Some(name) = &name {
            diesel::update(export_profiles::table.find(&profile_id))
                .set(export_profiles::name.eq(name))
                .execute(conn)?;
        }
        if let Some(options) = &options {
            diesel::update(export_profiles::table.find(&profile_id))
                .set(export_profiles::options.eq(options))
                .execute(conn)?;
        }
        diesel::update(export_profiles::table.find(&profile_id))
            .set(export_profiles::updated_at.eq(now_iso()))
            .execute(conn)?;
        Ok(export_profiles::table
            .find(&profile_id)
            .select(ExportProfile::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(view_of(row)))
}

#[utoipa::path(
    delete,
    path = "/api/export/profiles/{profile_id}",
    tag = "export",
    params(("profile_id" = String, Path)),
    responses(
        (status = 204, description = "Deleted"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_profile(
    State(state): State<AppState>,
    user: AuthUser,
    Path(profile_id): Path<String>,
) -> ApiResult<StatusCode> {
    if profile_id.starts_with("builtin:") {
        return Err(ApiError::BadRequest(
            "the built-in profiles cannot be deleted".into(),
        ));
    }
    blocking(&state.pool, move |conn| {
        let gone = diesel::delete(
            export_profiles::table
                .filter(export_profiles::id.eq(&profile_id))
                .filter(export_profiles::user_id.eq(&user.id)),
        )
        .execute(conn)?;
        if gone == 0 {
            return Err(ApiError::NotFound("no such profile".into()));
        }
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Last used, and Export again ─────────────────────────────────────────────

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LastUsed {
    pub format: String,
    pub options: ExportOptions,
    pub used_at: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LastUsedResponse {
    /// The most recent export of any format, which "Export again" repeats.
    pub latest: Option<LastUsed>,
    /// The latest of each format, which the dialog opens with.
    pub by_format: Vec<LastUsed>,
}

fn last_of(format: String, options: String, used_at: String) -> LastUsed {
    let raw: serde_json::Value = serde_json::from_str(&options).unwrap_or_default();
    let (options, notes) = ExportOptions::repaired(raw);
    LastUsed {
        format,
        options,
        used_at,
        notes,
    }
}

fn load_last(
    conn: &mut crate::db::Conn,
    work_id: &str,
    user: &AuthUser,
) -> ApiResult<LastUsedResponse> {
    support::work_for_owner(conn, work_id, user)?;
    let rows: Vec<(String, String, String)> = export_last_options::table
        .filter(export_last_options::work_id.eq(work_id))
        .order(export_last_options::used_at.desc())
        .select((
            export_last_options::format,
            export_last_options::options,
            export_last_options::used_at,
        ))
        .load(conn)?;
    let mut all: Vec<LastUsed> = rows
        .into_iter()
        .map(|(f, o, u)| last_of(f, o, u))
        .collect();
    let latest = all.first().map(|l| LastUsed {
        format: l.format.clone(),
        options: l.options.clone(),
        used_at: l.used_at.clone(),
        notes: l.notes.clone(),
    });
    all.sort_by(|a, b| a.format.cmp(&b.format));
    Ok(LastUsedResponse {
        latest,
        by_format: all,
    })
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/export/last",
    tag = "export",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "The settings last used", body = LastUsedResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn last_used(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<Json<LastUsedResponse>> {
    let last = blocking(&state.pool, move |conn| load_last(conn, &work_id, &user)).await?;
    Ok(Json(last))
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/export/again",
    tag = "export",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "The working draft, exported as it last was", content_type = "application/octet-stream"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn export_again(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<axum::response::Response> {
    let id = work_id.clone();
    let u = user.clone();
    let last = blocking(&state.pool, move |conn| load_last(conn, &id, &u)).await?;
    let Some(latest) = last.latest else {
        return Err(ApiError::BadRequest(
            "You have not exported this book yet. Choose Book → Export… first, then Export again \
             will repeat it."
                .into(),
        ));
    };
    let options = checked(latest.options)?;
    let resolved = resolve_draft(&state, user.clone(), work_id).await?;
    let file = build_file(&resolved, &options)?;
    remember(&state, &user, &resolved, &options, &file).await?;
    respond(file)
}

// ── History ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    pub id: String,
    pub created_at: String,
    pub format: String,
    pub profile: Option<String>,
    pub options_hash: String,
    pub filename: String,
    pub byte_size: i32,
    pub version_id: Option<String>,
    pub version_label: Option<String>,
    /// Whether the file can be made again: it came from a frozen version, and
    /// did not depend on comments, which can change.
    pub can_download: bool,
    pub options: ExportOptions,
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/export/history",
    tag = "export",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "What was exported, newest first", body = Vec<HistoryItem>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn history(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Vec<HistoryItem>>> {
    let items = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &work_id, &user)?;
        let rows: Vec<ExportHistoryRow> = export_history::table
            .filter(export_history::work_id.eq(&work_id))
            .order(export_history::created_at.desc())
            .limit(HISTORY_LIMIT)
            .select(ExportHistoryRow::as_select())
            .load(conn)?;
        let version_ids: Vec<String> = rows.iter().filter_map(|r| r.version_id.clone()).collect();
        let labels: Vec<(String, String)> = versions::table
            .filter(versions::id.eq_any(&version_ids))
            .select((versions::id, versions::label))
            .load(conn)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let raw: serde_json::Value = serde_json::from_str(&row.options).unwrap_or_default();
                let (options, _) = ExportOptions::repaired(raw);
                let label = row.version_id.as_ref().and_then(|id| {
                    labels
                        .iter()
                        .find(|(v, _)| v == id)
                        .map(|(_, label)| label.clone())
                });
                HistoryItem {
                    can_download: row.version_id.is_some()
                        && options.content.comments == CommentsMode::Off
                        && label.is_some(),
                    id: row.id,
                    created_at: row.created_at,
                    format: row.format,
                    profile: row.profile_name,
                    options_hash: row.options_hash,
                    filename: row.filename,
                    byte_size: row.byte_size,
                    version_id: row.version_id,
                    version_label: label,
                    options,
                }
            })
            .collect())
    })
    .await?;
    Ok(Json(items))
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/export/history/{history_id}/download",
    tag = "export",
    params(("work_id" = String, Path), ("history_id" = String, Path)),
    responses(
        (status = 200, description = "The file, made again from the frozen version", content_type = "application/octet-stream"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn download_again(
    State(state): State<AppState>,
    user: AuthUser,
    Path((work_id, history_id)): Path<(String, String)>,
) -> ApiResult<axum::response::Response> {
    let u = user.clone();
    let row = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &work_id, &u)?;
        export_history::table
            .filter(export_history::id.eq(&history_id))
            .filter(export_history::work_id.eq(&work_id))
            .select(ExportHistoryRow::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("no such export".into()))
    })
    .await?;

    let Some(version_id) = row.version_id.clone() else {
        return Err(ApiError::BadRequest(
            "This export was made from the working draft, which has changed since, so only the \
             record is kept. Export the draft again, or export a version for a copy you can \
             download later."
                .into(),
        ));
    };
    let raw: serde_json::Value = serde_json::from_str(&row.options).unwrap_or_default();
    let (options, _) = ExportOptions::repaired(raw);
    if options.content.comments != CommentsMode::Off {
        return Err(ApiError::BadRequest(
            "This export carried your comments, which may have changed since, so it cannot be \
             made again exactly. Export the version again."
                .into(),
        ));
    }
    let resolved = resolve_version(&state, user, version_id).await?;
    let file = build_file(&resolved, &options)?;
    let identical = hex::encode(Sha256::digest(&file.bytes)) == row.content_hash;
    let mut response = respond(file)?;
    response.headers_mut().insert(
        "x-export-identical",
        HeaderValue::from_static(if identical { "true" } else { "false" }),
    );
    Ok(response)
}

// ── All formats at once ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BundleRequest {
    /// Any subset of `docx`, `pdf`, `epub`, `html`, `txt`.
    pub formats: Vec<Format>,
    /// Settings shared by every file; the per-format blocks (`pdf`, `epub`,
    /// `html`) apply to their own format only.
    #[serde(default)]
    pub options: ExportOptions,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BundleFile {
    pub name: String,
    pub format: String,
    pub bytes: usize,
    pub warnings: Vec<Issue>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BundleManifest {
    pub generated_at: String,
    pub files: Vec<BundleFile>,
}

/// The options for one format of a bundle: the shared settings, with only the
/// block that belongs to that format.
fn options_for(base: &ExportOptions, format: Format) -> ExportOptions {
    let mut options = base.clone();
    options.format = format;
    options.pdf = (format == Format::Pdf).then(|| base.pdf.clone()).flatten();
    options.epub = (format == Format::Epub).then(|| base.epub.clone()).flatten();
    options.html = (format == Format::Html).then(|| base.html.clone()).flatten();
    options
}

fn bundle(
    resolved: &Resolved,
    request: &BundleRequest,
) -> ApiResult<(Vec<u8>, Vec<(ExportOptions, super::export::BuiltFile)>, String)> {
    if request.formats.is_empty() {
        return Err(ApiError::BadRequest(
            "choose at least one format to export".into(),
        ));
    }
    let mut formats: Vec<Format> = Vec::new();
    for f in &request.formats {
        if !formats.contains(f) {
            formats.push(*f);
        }
    }

    let mut built: Vec<(ExportOptions, super::export::BuiltFile)> = Vec::new();
    for format in formats {
        let options = options_for(&request.options, format);
        checked(options.clone())?;
        let file = build_file(resolved, &options).map_err(|e| match e {
            ApiError::BadRequest(m) => {
                ApiError::BadRequest(format!("{}: {m}", format.extension().to_uppercase()))
            }
            other => other,
        })?;
        built.push((options, file));
    }

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut names: Vec<String> = Vec::new();
    let mut manifest = BundleManifest {
        generated_at: now_iso(),
        files: Vec::new(),
    };
    for (options, file) in &built {
        // Two files never share a name, whatever the template made of them.
        let mut name = file.filename.clone();
        let mut n = 2;
        while names.contains(&name) {
            let (stem, ext) = file.filename.rsplit_once('.').unwrap_or((&file.filename, ""));
            name = format!("{stem}-{n}.{ext}");
            n += 1;
        }
        names.push(name.clone());
        zip.start_file(&name, deflated)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        zip.write_all(&file.bytes)?;
        manifest.files.push(BundleFile {
            name,
            format: options.format.extension().into(),
            bytes: file.bytes.len(),
            warnings: file
                .issues
                .iter()
                .filter(|i| i.severity != Severity::Info)
                .cloned()
                .collect(),
        });
    }
    zip.start_file("manifest.json", deflated)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    let bytes = zip
        .finish()
        .map_err(|e| ApiError::internal(e.to_string()))?
        .into_inner();

    let title = built
        .first()
        .map(|(_, f)| f.filename.rsplit_once('.').map(|(s, _)| s.to_string()).unwrap_or_default())
        .unwrap_or_default();
    Ok((bytes, built, format!("{title}-all-formats")))
}

async fn bundle_response(
    state: &AppState,
    user: &AuthUser,
    resolved: &Resolved,
    request: BundleRequest,
) -> ApiResult<axum::response::Response> {
    let (bytes, built, stem) = bundle(resolved, &request)?;
    for (options, file) in &built {
        remember(state, user, resolved, options, file).await?;
    }
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            assemble::safe_filename(&stem, "zip")
        ))
        .map_err(|e| ApiError::internal(e.to_string()))?,
    );
    Ok((StatusCode::OK, headers, bytes).into_response())
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/export/bundle",
    tag = "export",
    params(("work_id" = String, Path)),
    request_body = BundleRequest,
    responses(
        (status = 200, description = "A zip of every format, with a manifest", content_type = "application/zip"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn bundle_draft(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<BundleRequest>,
) -> ApiResult<axum::response::Response> {
    let resolved = resolve_draft(&state, user.clone(), work_id).await?;
    bundle_response(&state, &user, &resolved, body).await
}

#[utoipa::path(
    post,
    path = "/api/versions/{version_id}/export/bundle",
    tag = "export",
    params(("version_id" = String, Path)),
    request_body = BundleRequest,
    responses(
        (status = 200, description = "A zip of every format, with a manifest", content_type = "application/zip"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn bundle_version(
    State(state): State<AppState>,
    user: AuthUser,
    Path(version_id): Path<String>,
    Json(body): Json<BundleRequest>,
) -> ApiResult<axum::response::Response> {
    let resolved = resolve_version(&state, user.clone(), version_id).await?;
    bundle_response(&state, &user, &resolved, body).await
}

// ── Preview ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct PreviewQuery {
    /// For a PDF preview: how many pages to lay out. Default 6.
    #[serde(default)]
    pub pages: Option<usize>,
}

/// The same book, as a page you can look at: the first pages for a PDF, a web
/// page for everything else, so a change to the layout can be seen without a
/// download. Nothing is recorded in the history.
fn preview(
    resolved: &Resolved,
    options: &ExportOptions,
    pages: usize,
) -> ApiResult<axum::response::Response> {
    let report = assemble::preflight(resolved, options);
    if let Some(error) = report.issues.iter().find(|i| i.severity == Severity::Error) {
        return Err(ApiError::BadRequest(error.message.clone()));
    }
    let prepared = assemble::prepare(resolved, options);
    let (bytes, mime) = if options.format == Format::Pdf {
        (
            crate::export::pdf::write_limited(&prepared.manuscript, options, Some(pages)),
            "application/pdf",
        )
    } else {
        // A styled page whatever the format, so it shows the typography.
        let mut shown = options.clone();
        shown.html = Some(HtmlOptions {
            mode: HtmlMode::Styled,
        });
        (
            crate::export::html::write(&prepared.manuscript, &shown).into_bytes(),
            "text/html; charset=utf-8",
        )
    };
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(mime));
    // Inline, so it opens in a tab rather than downloading; and sandboxed, since
    // it is built from the writer's own text.
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; img-src data:"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((StatusCode::OK, headers, bytes).into_response())
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/export/preview",
    tag = "export",
    params(("work_id" = String, Path), PreviewQuery),
    request_body = ExportOptions,
    responses(
        (status = 200, description = "A page to look at", content_type = "text/html"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn preview_draft(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Query(query): Query<PreviewQuery>,
    Json(options): Json<ExportOptions>,
) -> ApiResult<axum::response::Response> {
    let options = checked(options)?;
    let resolved = resolve_draft(&state, user, work_id).await?;
    preview(&resolved, &options, query.pages.unwrap_or(6))
}

#[utoipa::path(
    post,
    path = "/api/versions/{version_id}/export/preview",
    tag = "export",
    params(("version_id" = String, Path), PreviewQuery),
    request_body = ExportOptions,
    responses(
        (status = 200, description = "A page to look at", content_type = "text/html"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn preview_version(
    State(state): State<AppState>,
    user: AuthUser,
    Path(version_id): Path<String>,
    Query(query): Query<PreviewQuery>,
    Json(options): Json<ExportOptions>,
) -> ApiResult<axum::response::Response> {
    let options = checked(options)?;
    let resolved = resolve_version(&state, user, version_id).await?;
    preview(&resolved, &options, query.pages.unwrap_or(6))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_profile_is_valid_and_named() {
        let all = builtin_profiles();
        assert_eq!(all.len(), 6);
        let mut ids = std::collections::HashSet::new();
        for (id, name, description, options) in all {
            assert!(ids.insert(id), "duplicate id {id}");
            assert!(!name.is_empty() && !description.is_empty());
            options.validate().unwrap_or_else(|e| panic!("{id}: {e}"));
            // A profile is presentation: no selection, no overrides.
            assert_eq!(options.as_profile().content.scope, options.content.scope);
        }
    }

    #[test]
    fn only_the_working_copy_includes_comments() {
        for (id, _, _, options) in builtin_profiles() {
            let has = options.content.comments != CommentsMode::Off;
            assert_eq!(has, id == "working-copy", "{id}");
        }
    }

    #[test]
    fn a_bundle_gives_each_format_its_own_block_only() {
        let base = ExportOptions {
            pdf: Some(Default::default()),
            epub: Some(EpubOptions::default()),
            html: Some(HtmlOptions { mode: HtmlMode::Clean }),
            ..Default::default()
        };
        let docx = options_for(&base, Format::Docx);
        assert!(docx.pdf.is_none() && docx.epub.is_none() && docx.html.is_none());
        assert!(docx.validate().is_ok());
        let epub = options_for(&base, Format::Epub);
        assert!(epub.epub.is_some() && epub.pdf.is_none() && epub.html.is_none());
        assert!(epub.validate().is_ok());
        let html = options_for(&base, Format::Html);
        assert_eq!(html.html.unwrap().mode, HtmlMode::Clean);
    }

    #[test]
    fn profile_names_are_checked() {
        assert!(clean_name("  ").is_err());
        assert!(clean_name(&"x".repeat(101)).is_err());
        assert_eq!(clean_name(" Agent copy ").unwrap(), "Agent copy");
    }

    #[test]
    fn an_old_profile_is_repaired_and_the_writer_told() {
        let raw = serde_json::json!({
            "format": "docx",
            "style": {"preset": "book", "font": "papyrus"},
            "content": {"scope": {"kind": "all"}, "noLongerASetting": true}
        });
        let (options, notes) = ExportOptions::repaired(raw);
        assert_eq!(options.style.font, None);
        assert_eq!(options.style.preset, StylePreset::Book);
        assert!(notes.iter().any(|n| n.contains("papyrus") || n.contains("content")));
    }
}
