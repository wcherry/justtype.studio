//! Versions are immutable snapshots of a whole work.
//!
//! Lifecycle: `draft` -> `published` -> `final`. A draft snapshot is a private
//! checkpoint; publishing makes it eligible to be offered to reviewers;
//! finalising freezes it as the work's canonical text. Snapshots are never
//! edited — the working copy moves on and the next snapshot captures it.
use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::{AuthUser, MaybeAuthUser};
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{version_entries, versions, works};
use crate::domain::diff::{self, DiffSummary, SceneChangeKind, SceneDiff};
use crate::domain::{doc, new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, Page};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateVersionRequest {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffQuery {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionDetail {
    #[serde(flatten)]
    pub version: Version,
    pub entries: Vec<VersionEntry>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VersionContent {
    #[serde(flatten)]
    pub version: Version,
    pub chapters: Vec<ContentChapter>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContentChapter {
    pub chapter_id: String,
    pub title: String,
    pub position: i32,
    pub scenes: Vec<ContentScene>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContentScene {
    pub scene_id: String,
    pub title: String,
    pub position: i32,
    pub word_count: i32,
    #[schema(value_type = crate::domain::doc::DocNode)]
    pub content: Value,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiffResponse {
    pub from: Version,
    pub to: Version,
    pub summary: DiffSummary,
    pub scenes: Vec<SceneDiff>,
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/versions",
    tag = "versions",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = Vec<Version>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Query(page): Query<Page>,
) -> ApiResult<Json<Vec<Version>>> {
    let (limit, offset) = page.clamped();
    let rows = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &work_id, &user)?;
        Ok(versions::table
            .filter(versions::work_id.eq(&work_id))
            .order(versions::number.desc())
            .limit(limit)
            .offset(offset)
            .select(Version::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(Json(rows))
}

/// Freeze the current working copy. Files are copied before any row is written
/// so a half-copied snapshot never becomes visible.
#[utoipa::path(
    post,
    path = "/api/works/{work_id}/versions",
    tag = "versions",
    params(("work_id" = String, Path)),
    request_body = CreateVersionRequest,
    responses(
        (status = 200, description = "Success", body = VersionDetail),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<CreateVersionRequest>,
) -> ApiResult<Json<VersionDetail>> {
    let guard_user = user.clone();
    let guard_work = work_id.clone();
    let outline = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &guard_work, &guard_user)?;
        super::works::load_outline(conn, &guard_work)
    })
    .await?;

    if outline.chapters.iter().all(|c| c.scenes.is_empty()) {
        return Err(ApiError::BadRequest(
            "there is nothing to snapshot: add at least one scene".into(),
        ));
    }

    let version_id = new_id();
    let now = now_iso();
    let mut entries: Vec<NewVersionEntry> = Vec::new();
    let mut manifest_chapters: Vec<Value> = Vec::new();
    let mut total_words = 0;

    for chapter in &outline.chapters {
        let mut manifest_scenes = Vec::new();
        for scene in &chapter.scenes {
            let content = state.store.read_working_scene(&work_id, &scene.id).await?;
            state
                .store
                .write_version_scene(&work_id, &version_id, &scene.id, &content)
                .await?;
            total_words += scene.word_count;
            entries.push(NewVersionEntry {
                id: new_id(),
                version_id: version_id.clone(),
                chapter_id: chapter.chapter.id.clone(),
                chapter_title: chapter.chapter.title.clone(),
                chapter_position: chapter.chapter.position,
                scene_id: scene.id.clone(),
                scene_title: scene.title.clone(),
                scene_position: scene.position,
                content_hash: scene.content_hash.clone(),
                word_count: scene.word_count,
            });
            manifest_scenes.push(json!({
                "sceneId": scene.id,
                "title": scene.title,
                "position": scene.position,
                "wordCount": scene.word_count,
                "contentHash": scene.content_hash,
            }));
        }
        manifest_chapters.push(json!({
            "chapterId": chapter.chapter.id,
            "title": chapter.chapter.title,
            "position": chapter.chapter.position,
            "scenes": manifest_scenes,
        }));
    }

    let label = body
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let db_version_id = version_id.clone();
    let db_work_id = work_id.clone();
    let created_by = user.id.clone();
    let created_now = now.clone();
    let detail = blocking(&state.pool, move |conn| {
        let previous: Option<Version> = versions::table
            .filter(versions::work_id.eq(&db_work_id))
            .order(versions::number.desc())
            .select(Version::as_select())
            .first(conn)
            .optional()?;
        let number = previous.as_ref().map(|v| v.number + 1).unwrap_or(1);
        let label = label.unwrap_or_else(|| format!("v{number}"));

        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::insert_into(versions::table)
                .values(&NewVersion {
                    id: db_version_id.clone(),
                    work_id: db_work_id.clone(),
                    number,
                    label,
                    state: "draft".into(),
                    parent_version_id: previous.as_ref().map(|v| v.id.clone()),
                    created_by,
                    notes: body.notes.clone(),
                    word_count: total_words,
                    created_at: created_now.clone(),
                    published_at: None,
                    finalized_at: None,
                })
                .execute(conn)?;
            diesel::insert_into(version_entries::table)
                .values(&entries)
                .execute(conn)?;
            Ok(())
        })?;

        load_detail(conn, &db_version_id)
    })
    .await?;

    state
        .store
        .write_version_manifest(
            &work_id,
            &version_id,
            &json!({
                "versionId": version_id,
                "workId": work_id,
                "number": detail.version.number,
                "label": detail.version.label,
                "createdAt": now,
                "wordCount": total_words,
                "chapters": manifest_chapters,
            }),
        )
        .await?;

    Ok(Json(detail))
}

fn load_detail(conn: &mut crate::db::Conn, version_id: &str) -> ApiResult<VersionDetail> {
    let version: Version = versions::table
        .find(version_id)
        .select(Version::as_select())
        .first(conn)?;
    let entries: Vec<VersionEntry> = version_entries::table
        .filter(version_entries::version_id.eq(version_id))
        .order((
            version_entries::chapter_position.asc(),
            version_entries::scene_position.asc(),
        ))
        .select(VersionEntry::as_select())
        .load(conn)?;
    Ok(VersionDetail { version, entries })
}

#[utoipa::path(
    get,
    path = "/api/versions/{version_id}",
    tag = "versions",
    params(("version_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = VersionDetail),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(version_id): Path<String>,
) -> ApiResult<Json<VersionDetail>> {
    let detail = blocking(&state.pool, move |conn| {
        support::version_for_owner(conn, &version_id, &user)?;
        load_detail(conn, &version_id)
    })
    .await?;
    Ok(Json(detail))
}

/// The whole frozen manuscript. Authors always get their own; readers reach a
/// version through a publication instead.
#[utoipa::path(
    get,
    path = "/api/versions/{version_id}/content",
    tag = "versions",
    params(("version_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = VersionContent),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn content(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
    Path(version_id): Path<String>,
) -> ApiResult<Json<VersionContent>> {
    let user = user.ok_or_else(|| ApiError::Unauthorized("sign in to read a version".into()))?;
    let detail = blocking(&state.pool, move |conn| {
        support::version_for_owner(conn, &version_id, &user)?;
        load_detail(conn, &version_id)
    })
    .await?;
    let content = assemble_content(&state, detail).await?;
    Ok(Json(content))
}

pub(crate) async fn assemble_content(
    state: &AppState,
    detail: VersionDetail,
) -> ApiResult<VersionContent> {
    let work_id = detail.version.work_id.clone();
    let version_id = detail.version.id.clone();
    let mut chapters: Vec<ContentChapter> = Vec::new();

    for entry in &detail.entries {
        let content = state
            .store
            .read_version_scene(&work_id, &version_id, &entry.scene_id)
            .await
            .unwrap_or_else(|_| doc::empty_doc());
        let scene = ContentScene {
            scene_id: entry.scene_id.clone(),
            title: entry.scene_title.clone(),
            position: entry.scene_position,
            word_count: entry.word_count,
            content,
        };
        match chapters.last_mut() {
            Some(last) if last.chapter_id == entry.chapter_id => last.scenes.push(scene),
            _ => chapters.push(ContentChapter {
                chapter_id: entry.chapter_id.clone(),
                title: entry.chapter_title.clone(),
                position: entry.chapter_position,
                scenes: vec![scene],
            }),
        }
    }

    Ok(VersionContent {
        version: detail.version,
        chapters,
    })
}

#[utoipa::path(
    post,
    path = "/api/versions/{version_id}/publish",
    tag = "versions",
    params(("version_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = Version),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn publish(
    State(state): State<AppState>,
    user: AuthUser,
    Path(version_id): Path<String>,
) -> ApiResult<Json<Version>> {
    transition(state, user, version_id, "published").await
}

#[utoipa::path(
    post,
    path = "/api/versions/{version_id}/finalize",
    tag = "versions",
    params(("version_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = Version),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn finalize(
    State(state): State<AppState>,
    user: AuthUser,
    Path(version_id): Path<String>,
) -> ApiResult<Json<Version>> {
    transition(state, user, version_id, "final").await
}

async fn transition(
    state: AppState,
    user: AuthUser,
    version_id: String,
    target: &'static str,
) -> ApiResult<Json<Version>> {
    let version = blocking(&state.pool, move |conn| {
        let (version, work) = support::version_for_owner(conn, &version_id, &user)?;
        // A draft may go straight to final: not every manuscript wants an
        // intermediate review round.
        let allowed = matches!(
            (version.state.as_str(), target),
            ("draft", "published") | ("published", "final") | ("draft", "final")
        );
        if !allowed {
            return Err(ApiError::Conflict(format!(
                "cannot move a {} version to {target}",
                version.state
            )));
        }
        let now = now_iso();
        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::update(versions::table.find(&version.id))
                .set(versions::state.eq(target))
                .execute(conn)?;
            if target == "published" {
                diesel::update(versions::table.find(&version.id))
                    .set(versions::published_at.eq(&now))
                    .execute(conn)?;
            } else {
                diesel::update(versions::table.find(&version.id))
                    .set(versions::finalized_at.eq(&now))
                    .execute(conn)?;
                if version.published_at.is_none() {
                    diesel::update(versions::table.find(&version.id))
                        .set(versions::published_at.eq(&now))
                        .execute(conn)?;
                }
            }
            diesel::update(works::table.find(&work.id))
                .set((
                    works::current_version_id.eq(&version.id),
                    works::status.eq(if target == "final" {
                        "final"
                    } else {
                        "published"
                    }),
                    works::updated_at.eq(&now),
                ))
                .execute(conn)?;
            Ok(())
        })?;
        Ok(versions::table
            .find(&version.id)
            .select(Version::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(version))
}

/// Scene-level comparison of two snapshots of the same work.
#[utoipa::path(
    get,
    path = "/api/works/{work_id}/versions/diff",
    tag = "versions",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = DiffResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn diff(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Query(query): Query<DiffQuery>,
) -> ApiResult<Json<DiffResponse>> {
    let (from_detail, to_detail) = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &work_id, &user)?;
        let from = load_detail(conn, &query.from)?;
        let to = load_detail(conn, &query.to)?;
        if from.version.work_id != work_id || to.version.work_id != work_id {
            return Err(ApiError::BadRequest(
                "both versions must belong to this work".into(),
            ));
        }
        Ok((from, to))
    })
    .await?;

    let mut summary = DiffSummary::default();
    let mut scene_diffs: Vec<SceneDiff> = Vec::new();

    for to_entry in &to_detail.entries {
        let from_entry = from_detail
            .entries
            .iter()
            .find(|e| e.scene_id == to_entry.scene_id);

        let (kind, changes) = match from_entry {
            None => {
                summary.scenes_added += 1;
                summary.words_added += to_entry.word_count;
                (SceneChangeKind::Added, Vec::new())
            }
            Some(before) if before.content_hash == to_entry.content_hash => {
                let moved = before.scene_position != to_entry.scene_position
                    || before.chapter_id != to_entry.chapter_id;
                if moved {
                    summary.scenes_moved += 1;
                    (SceneChangeKind::Moved, Vec::new())
                } else {
                    (SceneChangeKind::Unchanged, Vec::new())
                }
            }
            Some(before) => {
                summary.scenes_modified += 1;
                let delta = to_entry.word_count - before.word_count;
                if delta >= 0 {
                    summary.words_added += delta;
                } else {
                    summary.words_removed += -delta;
                }
                let before_doc = state
                    .store
                    .read_version_scene(
                        &from_detail.version.work_id,
                        &from_detail.version.id,
                        &before.scene_id,
                    )
                    .await
                    .unwrap_or_else(|_| doc::empty_doc());
                let after_doc = state
                    .store
                    .read_version_scene(
                        &to_detail.version.work_id,
                        &to_detail.version.id,
                        &to_entry.scene_id,
                    )
                    .await
                    .unwrap_or_else(|_| doc::empty_doc());
                (
                    SceneChangeKind::Modified,
                    diff::diff_text(&doc::plain_text(&before_doc), &doc::plain_text(&after_doc)),
                )
            }
        };

        scene_diffs.push(SceneDiff {
            scene_id: to_entry.scene_id.clone(),
            scene_title: to_entry.scene_title.clone(),
            chapter_title: to_entry.chapter_title.clone(),
            kind,
            words_before: from_entry.map(|e| e.word_count).unwrap_or(0),
            words_after: to_entry.word_count,
            changes,
        });
    }

    for from_entry in &from_detail.entries {
        if !to_detail
            .entries
            .iter()
            .any(|e| e.scene_id == from_entry.scene_id)
        {
            summary.scenes_removed += 1;
            summary.words_removed += from_entry.word_count;
            scene_diffs.push(SceneDiff {
                scene_id: from_entry.scene_id.clone(),
                scene_title: from_entry.scene_title.clone(),
                chapter_title: from_entry.chapter_title.clone(),
                kind: SceneChangeKind::Removed,
                words_before: from_entry.word_count,
                words_after: 0,
                changes: Vec::new(),
            });
        }
    }

    Ok(Json(DiffResponse {
        from: from_detail.version,
        to: to_detail.version,
        summary,
        scenes: scene_diffs,
    }))
}
