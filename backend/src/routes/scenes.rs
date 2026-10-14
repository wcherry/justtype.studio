use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{scene_revisions, scenes};
use crate::domain::enums::{ActivityKind, SceneStatus};
use crate::domain::{doc, new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::collaborators::record;
use super::support::{self, Ability, Page};
use super::works::ReorderRequest;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateSceneRequest {
    /// Left out or empty, it is untitled and shown as "Scene 2" for where it
    /// falls, until the writer names it.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub pov: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::doc::DocNode>)]
    pub content: Option<Value>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSceneRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub pov: Option<String>,
    /// A built-in status, or the key of one of the book's own.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    /// Free text: fiction rarely runs on the Gregorian calendar.
    #[serde(default)]
    pub story_date: Option<String>,
    /// A word target for this scene. Omit to leave it alone; send `null` to
    /// clear it. The nesting is what distinguishes those two — see
    /// `present_or_absent`.
    #[serde(default, deserialize_with = "present_or_absent")]
    #[schema(value_type = Option<i32>)]
    pub target_words: Option<Option<i32>>,
    /// Move the scene into a different chapter of the same work.
    #[serde(default)]
    pub chapter_id: Option<String>,
    /// Leave the scene out of the book (`true`) or put it back: an excluded
    /// scene is not exported, not in a version, and not in the word count.
    #[serde(default)]
    pub excluded: Option<bool>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PutContentRequest {
    #[schema(value_type = crate::domain::doc::DocNode)]
    pub content: Value,
    /// Autosaves pass `false`; explicit saves and idle-flushes pass `true`.
    #[serde(default = "default_true")]
    pub create_revision: bool,
    /// The `contentHash` the client believes it is editing on top of. When it
    /// no longer matches, someone else — another tab, another device, a queued
    /// offline save — has written in the meantime, and this save is refused
    /// rather than silently overwriting their work.
    ///
    /// Omitted only when there is nothing to protect: a scene the client has
    /// not read yet. To resolve a conflict deliberately, re-send with the hash
    /// the 409 reported, which says "I have seen their version and still mean
    /// this".
    #[serde(default)]
    pub base_hash: Option<String>,
    /// The writer's local date, `YYYY-MM-DD`, so this save counts toward the
    /// right day. Omitted, it is not recorded against any day at all: skipping
    /// a tally is better than crediting the wrong one.
    #[serde(default)]
    pub local_day: Option<String>,
}

fn default_true() -> bool {
    true
}

/// Distinguishes "the client sent null" from "the client did not mention it".
///
/// A plain `Option<Option<T>>` cannot: serde collapses an explicit `null` to
/// the outer `None`, which is the same thing absence produces. Running a
/// deserializer only when the key is present, and wrapping whatever it yields,
/// keeps the two apart — which is what lets a PATCH clear a nullable field
/// instead of only ever setting one.
fn present_or_absent<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(deserializer).map(Some)
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SceneWithContent {
    #[serde(flatten)]
    pub scene: Scene,
    #[schema(value_type = crate::domain::doc::DocNode)]
    pub content: Value,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub scene: Scene,
    pub revision_id: Option<String>,
    pub work_word_count: i32,
}

#[utoipa::path(
    post,
    path = "/api/chapters/{chapter_id}/scenes",
    tag = "scenes",
    params(("chapter_id" = String, Path)),
    request_body = CreateSceneRequest,
    responses(
        (status = 200, description = "Success", body = Scene),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(chapter_id): Path<String>,
    Json(body): Json<CreateSceneRequest>,
) -> ApiResult<Json<Scene>> {
    let content = body.content.clone().unwrap_or_else(doc::empty_doc);
    doc::validate(&content).map_err(ApiError::BadRequest)?;
    let content_for_db = content.clone();

    let scene = blocking(&state.pool, move |conn| {
        let (chapter, work) = support::chapter_for(conn, &chapter_id, &user, Ability::Write)?;
        let now = now_iso();
        let row = NewScene {
            id: new_id(),
            work_id: work.id.clone(),
            chapter_id: chapter.id.clone(),
            title: body.title.trim().to_string(),
            summary: body.summary.clone(),
            pov: body.pov.clone(),
            status: "drafting".into(),
            position: support::next_position(conn, &chapter.id)?,
            word_count: doc::word_count(&content_for_db),
            content_hash: doc::content_hash(&content_for_db),
            created_at: now.clone(),
            updated_at: now.clone(),
            target_words: None,
            location: None,
            story_date: None,
        };
        diesel::insert_into(scenes::table)
            .values(&row)
            .execute(conn)?;
        support::recount_work(conn, &work.id, &now)?;
        record(
            conn,
            &work.id,
            &user.id,
            ActivityKind::SceneAdded,
            Some(&row.id),
            &row.title,
        )?;
        Ok(scenes::table
            .find(&row.id)
            .select(Scene::as_select())
            .first(conn)?)
    })
    .await?;

    state
        .store
        .write_working_scene(&scene.work_id, &scene.id, &content)
        .await?;
    Ok(Json(scene))
}

#[utoipa::path(
    get,
    path = "/api/scenes/{scene_id}",
    tag = "scenes",
    params(("scene_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = SceneWithContent),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
) -> ApiResult<Json<SceneWithContent>> {
    let scene = blocking(&state.pool, move |conn| {
        let (scene, _) = support::scene_for(conn, &scene_id, &user, Ability::Read)?;
        Ok(scene)
    })
    .await?;
    let content = state
        .store
        .read_working_scene(&scene.work_id, &scene.id)
        .await?;
    Ok(Json(SceneWithContent { scene, content }))
}

#[utoipa::path(
    patch,
    path = "/api/scenes/{scene_id}",
    tag = "scenes",
    params(("scene_id" = String, Path)),
    request_body = UpdateSceneRequest,
    responses(
        (status = 200, description = "Success", body = Scene),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Json(body): Json<UpdateSceneRequest>,
) -> ApiResult<Json<Scene>> {
    let scene = blocking(&state.pool, move |conn| {
        let (scene, work) = support::scene_for(conn, &scene_id, &user, Ability::Write)?;
        // Before anything is written, so a refused status changes nothing.
        if let Some(status) = &body.status {
            if !super::statuses::accepts(conn, &work.id, status)? {
                return Err(ApiError::BadRequest(format!(
                    "status must be one of {}, or one of the book's own",
                    SceneStatus::options()
                )));
            }
        }
        let now = now_iso();
        if let Some(title) = &body.title {
            diesel::update(scenes::table.find(&scene.id))
                .set(scenes::title.eq(title.trim()))
                .execute(conn)?;
        }
        if let Some(summary) = &body.summary {
            diesel::update(scenes::table.find(&scene.id))
                .set(scenes::summary.eq(summary))
                .execute(conn)?;
        }
        if let Some(pov) = &body.pov {
            diesel::update(scenes::table.find(&scene.id))
                .set(scenes::pov.eq(pov))
                .execute(conn)?;
        }
        // Blank clears these, the way emptying the field in the UI means.
        if let Some(location) = &body.location {
            let value = location.trim();
            diesel::update(scenes::table.find(&scene.id))
                .set(scenes::location.eq((!value.is_empty()).then_some(value)))
                .execute(conn)?;
        }
        if let Some(story_date) = &body.story_date {
            let value = story_date.trim();
            diesel::update(scenes::table.find(&scene.id))
                .set(scenes::story_date.eq((!value.is_empty()).then_some(value)))
                .execute(conn)?;
        }
        if let Some(status) = &body.status {
            diesel::update(scenes::table.find(&scene.id))
                .set(scenes::status.eq(status))
                .execute(conn)?;
        }
        if let Some(target) = body.target_words {
            if target.is_some_and(|words| words <= 0) {
                return Err(ApiError::BadRequest(
                    "a scene target has to be a positive number of words".into(),
                ));
            }
            diesel::update(scenes::table.find(&scene.id))
                .set(scenes::target_words.eq(target))
                .execute(conn)?;
        }
        if let Some(excluded) = body.excluded {
            diesel::update(scenes::table.find(&scene.id))
                .set(scenes::excluded.eq(excluded))
                .execute(conn)?;
            support::recount_work(conn, &work.id, &now)?;
        }
        if let Some(target_chapter) = &body.chapter_id {
            let (chapter, _) = support::chapter_for(conn, target_chapter, &user, Ability::Write)?;
            if chapter.work_id != work.id {
                return Err(ApiError::BadRequest(
                    "a scene can only move within its own work".into(),
                ));
            }
            let position = support::next_position(conn, &chapter.id)?;
            diesel::update(scenes::table.find(&scene.id))
                .set((
                    scenes::chapter_id.eq(&chapter.id),
                    scenes::position.eq(position),
                ))
                .execute(conn)?;
        }
        diesel::update(scenes::table.find(&scene.id))
            .set(scenes::updated_at.eq(&now))
            .execute(conn)?;
        Ok(scenes::table
            .find(&scene.id)
            .select(Scene::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(scene))
}

/// The hot path: the editor PUTs the whole scene document. Content goes to
/// disk, counts and hashes go to SQLite, and an optional revision is kept so
/// the author can walk back through their own drafting history.
#[utoipa::path(
    put,
    path = "/api/scenes/{scene_id}/content",
    tag = "scenes",
    params(("scene_id" = String, Path)),
    request_body = PutContentRequest,
    responses(
        (status = 200, description = "Success", body = SaveResult),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn put_content(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Json(body): Json<PutContentRequest>,
) -> ApiResult<Json<SaveResult>> {
    doc::validate(&body.content).map_err(ApiError::BadRequest)?;
    let hash = doc::content_hash(&body.content);
    let words = doc::word_count(&body.content);

    let lookup_id = scene_id.clone();
    let lookup_user = user.clone();
    let scene = blocking(&state.pool, move |conn| {
        let (scene, _) = support::scene_for(conn, &lookup_id, &lookup_user, Ability::Write)?;
        Ok(scene)
    })
    .await?;

    if let Some(base) = &body.base_hash {
        // Equal hashes mean this save is a no-op against whatever is stored, so
        // it cannot be clobbering anything and is allowed through regardless.
        if base != &scene.content_hash && hash != scene.content_hash {
            return Err(ApiError::ConflictWith {
                message: "this scene changed somewhere else since you last loaded it".into(),
                details: serde_json::json!({
                    "sceneId": scene.id,
                    "currentHash": scene.content_hash,
                    "currentWordCount": scene.word_count,
                    "updatedAt": scene.updated_at,
                    "yourHash": hash,
                }),
            });
        }
    }

    let unchanged = scene.content_hash == hash;
    state
        .store
        .write_working_scene(&scene.work_id, &scene.id, &body.content)
        .await?;

    let revision_id = if body.create_revision && !unchanged {
        let revision_id = new_id();
        state
            .store
            .write_revision(&scene.work_id, &scene.id, &revision_id, &body.content)
            .await?;
        Some(revision_id)
    } else {
        None
    };

    let work_id = scene.work_id.clone();
    let previous_words = scene.word_count;
    let scene_id_db = scene.id.clone();
    let revision_for_db = revision_id.clone();
    let author_id = user.id.clone();
    let actor_id = user.id.clone();
    let scene_for_label = scene.clone();
    let local_day = body.local_day.clone();
    let (scene, work_word_count) = blocking(&state.pool, move |conn| {
        let now = now_iso();
        // Recorded inside the same closure as the word count it came from, so a
        // day's tally and the save that produced it cannot disagree.
        if let Some(day) = &local_day {
            super::goals::record_save(conn, &author_id, day, words - previous_words)?;
        }
        diesel::update(scenes::table.find(&scene_id_db))
            .set((
                scenes::content_hash.eq(&hash),
                scenes::word_count.eq(words),
                scenes::updated_at.eq(&now),
            ))
            .execute(conn)?;
        if let Some(revision_id) = revision_for_db {
            diesel::insert_into(scene_revisions::table)
                .values(&NewSceneRevision {
                    id: revision_id,
                    scene_id: scene_id_db.clone(),
                    work_id: work_id.clone(),
                    author_id,
                    content_hash: hash.clone(),
                    word_count: words,
                    created_at: now.clone(),
                })
                .execute(conn)?;
        }
        let total = support::recount_work(conn, &work_id, &now)?;
        if !unchanged {
            let label = support::scene_label(conn, &scene_for_label)?;
            record(
                conn,
                &work_id,
                &actor_id,
                ActivityKind::SceneEdited,
                Some(&scene_id_db),
                &label,
            )?;
        }
        let scene: Scene = scenes::table
            .find(&scene_id_db)
            .select(Scene::as_select())
            .first(conn)?;
        Ok((scene, total))
    })
    .await?;

    if !unchanged {
        crate::live::announce_save(&state, &scene, &user.id).await;
    }
    Ok(Json(SaveResult {
        scene,
        revision_id,
        work_word_count,
    }))
}

#[utoipa::path(
    delete,
    path = "/api/scenes/{scene_id}",
    tag = "scenes",
    params(("scene_id" = String, Path)),
    responses(
        (status = 200, description = "Success"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_scene(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (work_id, scene_id) = blocking(&state.pool, move |conn| {
        let (scene, work) = support::scene_for(conn, &scene_id, &user, Ability::Write)?;
        let label = support::scene_label(conn, &scene)?;
        diesel::delete(scenes::table.find(&scene.id)).execute(conn)?;
        record(
            conn,
            &work.id,
            &user.id,
            ActivityKind::SceneRemoved,
            Some(&scene.id),
            &label,
        )?;
        support::recount_work(conn, &work.id, &now_iso())?;
        Ok((work.id, scene.id))
    })
    .await?;
    state.store.delete_scene(&work_id, &scene_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// A copy of a scene, just after it: its text, details and notebook links,
/// under the same name with "(copy)" after it. Its history and comments stay
/// with the original.
#[utoipa::path(
    post,
    path = "/api/scenes/{scene_id}/duplicate",
    tag = "scenes",
    params(("scene_id" = String, Path)),
    responses(
        (status = 200, description = "The copy", body = Scene),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn duplicate(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
) -> ApiResult<Json<Scene>> {
    let (original, copy) = blocking(&state.pool, move |conn| {
        let (scene, work) = support::scene_for(conn, &scene_id, &user, Ability::Write)?;
        let now = now_iso();
        let id = new_id();
        conn.transaction::<_, ApiError, _>(|conn| {
            // Room for it just after the original.
            diesel::update(
                scenes::table
                    .filter(scenes::chapter_id.eq(&scene.chapter_id))
                    .filter(scenes::position.gt(scene.position)),
            )
            .set(scenes::position.eq(scenes::position + 1))
            .execute(conn)?;
            let title = if scene.title.trim().is_empty() {
                String::new()
            } else {
                format!("{} (copy)", scene.title.trim())
            };
            diesel::insert_into(scenes::table)
                .values(&NewScene {
                    id: id.clone(),
                    work_id: work.id.clone(),
                    chapter_id: scene.chapter_id.clone(),
                    title,
                    summary: scene.summary.clone(),
                    pov: scene.pov.clone(),
                    status: scene.status.clone(),
                    position: scene.position + 1,
                    word_count: scene.word_count,
                    content_hash: scene.content_hash.clone(),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    target_words: scene.target_words,
                    location: scene.location.clone(),
                    story_date: scene.story_date.clone(),
                })
                .execute(conn)?;
            diesel::update(scenes::table.find(&id))
                .set(scenes::excluded.eq(scene.excluded))
                .execute(conn)?;
            let links: Vec<String> = crate::db::schema::scene_notebook_links::table
                .filter(crate::db::schema::scene_notebook_links::scene_id.eq(&scene.id))
                .select(crate::db::schema::scene_notebook_links::entry_id)
                .load(conn)?;
            for entry_id in links {
                diesel::insert_into(crate::db::schema::scene_notebook_links::table)
                    .values(&NewSceneNotebookLink {
                        id: new_id(),
                        scene_id: id.clone(),
                        entry_id,
                        created_at: now.clone(),
                    })
                    .execute(conn)?;
            }
            support::recount_work(conn, &work.id, &now)?;
            Ok(())
        })?;
        let copy: Scene = scenes::table
            .find(&id)
            .select(Scene::as_select())
            .first(conn)?;
        let label = support::scene_label(conn, &copy)?;
        record(
            conn,
            &work.id,
            &user.id,
            ActivityKind::SceneAdded,
            Some(&copy.id),
            &label,
        )?;
        Ok((scene, copy))
    })
    .await?;
    let content = state
        .store
        .read_working_scene(&original.work_id, &original.id)
        .await
        .unwrap_or_else(|_| doc::empty_doc());
    state
        .store
        .write_working_scene(&copy.work_id, &copy.id, &content)
        .await?;
    Ok(Json(copy))
}

#[utoipa::path(
    post,
    path = "/api/chapters/{chapter_id}/scenes/reorder",
    tag = "scenes",
    params(("chapter_id" = String, Path)),
    request_body = ReorderRequest,
    responses(
        (status = 200, description = "Success", body = Vec<Scene>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn reorder(
    State(state): State<AppState>,
    user: AuthUser,
    Path(chapter_id): Path<String>,
    Json(body): Json<ReorderRequest>,
) -> ApiResult<Json<Vec<Scene>>> {
    let rows = blocking(&state.pool, move |conn| {
        let (chapter, _) = support::chapter_for(conn, &chapter_id, &user, Ability::Write)?;
        let now = now_iso();
        conn.transaction::<_, ApiError, _>(|conn| {
            for (index, id) in body.ids.iter().enumerate() {
                diesel::update(
                    scenes::table
                        .find(id)
                        .filter(scenes::chapter_id.eq(&chapter.id)),
                )
                .set((
                    scenes::position.eq(index as i32),
                    scenes::updated_at.eq(&now),
                ))
                .execute(conn)?;
            }
            Ok(())
        })?;
        Ok(scenes::table
            .filter(scenes::chapter_id.eq(&chapter.id))
            .order(scenes::position.asc())
            .select(Scene::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(Json(rows))
}

#[utoipa::path(
    get,
    path = "/api/scenes/{scene_id}/revisions",
    tag = "scenes",
    params(("scene_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = Vec<SceneRevision>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_revisions(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Query(page): Query<Page>,
) -> ApiResult<Json<Vec<SceneRevision>>> {
    let (limit, offset) = page.clamped();
    let rows = blocking(&state.pool, move |conn| {
        let (scene, _) = support::scene_for(conn, &scene_id, &user, Ability::Read)?;
        Ok(scene_revisions::table
            .filter(scene_revisions::scene_id.eq(&scene.id))
            .order(scene_revisions::created_at.desc())
            .limit(limit)
            .offset(offset)
            .select(SceneRevision::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevisionWithContent {
    #[serde(flatten)]
    pub revision: SceneRevision,
    #[schema(value_type = crate::domain::doc::DocNode)]
    pub content: Value,
    /// Paragraph-aligned diff against the scene as it stands now — what
    /// restoring this revision would change.
    pub blocks: Vec<crate::domain::diff::BlockDiff>,
}

#[utoipa::path(
    get,
    path = "/api/scenes/{scene_id}/revisions/{revision_id}",
    tag = "scenes",
    params(("scene_id" = String, Path), ("revision_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = RevisionWithContent),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_revision(
    State(state): State<AppState>,
    user: AuthUser,
    Path((scene_id, revision_id)): Path<(String, String)>,
) -> ApiResult<Json<RevisionWithContent>> {
    let revision_lookup = revision_id.clone();
    let (scene, revision) = blocking(&state.pool, move |conn| {
        let (scene, _) = support::scene_for(conn, &scene_id, &user, Ability::Read)?;
        let revision: SceneRevision = scene_revisions::table
            .find(&revision_lookup)
            .filter(scene_revisions::scene_id.eq(&scene.id))
            .select(SceneRevision::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("revision not found".into()))?;
        Ok((scene, revision))
    })
    .await?;

    let content = state
        .store
        .read_revision(&scene.work_id, &scene.id, &revision.id)
        .await?;
    let current = state
        .store
        .read_working_scene(&scene.work_id, &scene.id)
        .await?;
    let blocks = crate::domain::diff::diff_blocks(
        &super::versions::paragraphs_of(&content),
        &super::versions::paragraphs_of(&current),
    );

    Ok(Json(RevisionWithContent {
        revision,
        content,
        blocks,
    }))
}

/// Put an earlier revision back as the working copy.
///
/// The restore is itself a save: it writes a fresh revision first, so the text
/// being replaced stays reachable and restoring is undoable. Nothing in the
/// revision history is ever rewritten or removed.
#[utoipa::path(
    post,
    path = "/api/scenes/{scene_id}/revisions/{revision_id}/restore",
    tag = "scenes",
    params(("scene_id" = String, Path), ("revision_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = SaveResult),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn restore_revision(
    State(state): State<AppState>,
    user: AuthUser,
    Path((scene_id, revision_id)): Path<(String, String)>,
) -> ApiResult<Json<SaveResult>> {
    let lookup_user = user.clone();
    let lookup_revision = revision_id.clone();
    let (scene, revision) = blocking(&state.pool, move |conn| {
        let (scene, _) = support::scene_for(conn, &scene_id, &lookup_user, Ability::Write)?;
        let revision: SceneRevision = scene_revisions::table
            .find(&lookup_revision)
            .filter(scene_revisions::scene_id.eq(&scene.id))
            .select(SceneRevision::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("revision not found".into()))?;
        Ok((scene, revision))
    })
    .await?;

    let content = state
        .store
        .read_revision(&scene.work_id, &scene.id, &revision.id)
        .await?;
    doc::validate(&content)
        .map_err(|e| ApiError::internal(format!("stored revision is invalid: {e}")))?;

    // Routed back through the ordinary save path so restoring cannot drift from
    // saving: same hashing, same revision bookkeeping, same word recount. The
    // base hash is the scene as it stands, because a restore is a deliberate
    // replacement of exactly that.
    put_content(
        State(state),
        user,
        Path(scene.id.clone()),
        Json(PutContentRequest {
            content,
            create_revision: true,
            base_hash: Some(scene.content_hash.clone()),
            // Deliberately uncounted: restoring is recovering, not writing, and
            // rolling back to a longer draft should not read as a day's work.
            local_day: None,
        }),
    )
    .await
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SplitSceneRequest {
    /// Index of the first top-level block that belongs to the *new* scene.
    /// Blocks, not characters: the cut is between paragraphs, which is how a
    /// writer thinks about "this should have been two scenes".
    pub at_block: usize,
    /// Title for the new scene. Defaults to the original's, with a suffix.
    #[serde(default)]
    pub title: Option<String>,
    /// The `contentHash` the split was decided against. Required, unlike on an
    /// ordinary save: splitting rewrites prose in two places, and doing that to
    /// a document someone else has edited would be much worse than refusing.
    pub base_hash: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SplitSceneResponse {
    /// The original scene, now holding only the blocks before the cut.
    pub first: Scene,
    /// The new scene, holding the rest.
    pub second: Scene,
}

/// Cuts one scene into two at a block boundary.
///
/// The new scene lands immediately after the original and everything below it
/// shifts down, so the reading order is unchanged. Nothing is lost: every block
/// ends up in exactly one of the two, which is what makes `merge` its inverse.
#[utoipa::path(
    post,
    path = "/api/scenes/{scene_id}/split",
    tag = "scenes",
    params(("scene_id" = String, Path)),
    request_body = SplitSceneRequest,
    responses(
        (status = 200, description = "Both halves", body = SplitSceneResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn split_scene(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Json(body): Json<SplitSceneRequest>,
) -> ApiResult<Json<SplitSceneResponse>> {
    let lookup_user = user.clone();
    let lookup_id = scene_id.clone();
    let scene = blocking(&state.pool, move |conn| {
        let (scene, _) = support::scene_for(conn, &lookup_id, &lookup_user, Ability::Write)?;
        Ok(scene)
    })
    .await?;

    if scene.content_hash != body.base_hash {
        return Err(ApiError::ConflictWith {
            message: "this scene changed since you decided where to split it".into(),
            details: serde_json::json!({
                "sceneId": scene.id,
                "currentHash": scene.content_hash,
                "currentWordCount": scene.word_count,
                "updatedAt": scene.updated_at,
                "yourHash": body.base_hash,
            }),
        });
    }

    let content = state
        .store
        .read_working_scene(&scene.work_id, &scene.id)
        .await?;
    let blocks = doc::block_count(&content);
    if body.at_block == 0 || body.at_block >= blocks {
        return Err(ApiError::BadRequest(format!(
            "atBlock must fall inside the scene: expected 1..{blocks}, got {}",
            body.at_block
        )));
    }

    let (first_doc, second_doc) = doc::split_doc(&content, body.at_block);
    let new_id_value = new_id();
    let title = body
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        // An untitled scene's second half is untitled too, and takes its own
        // number from where it lands.
        .unwrap_or_else(|| {
            if scene.title.trim().is_empty() {
                String::new()
            } else {
                format!("{} (continued)", scene.title)
            }
        });

    // Files first: a scene row pointing at a document that was never written
    // would read as lost work, where an orphaned file is merely untidy.
    state
        .store
        .write_working_scene(&scene.work_id, &new_id_value, &second_doc)
        .await?;
    state
        .store
        .write_working_scene(&scene.work_id, &scene.id, &first_doc)
        .await?;

    let work_id = scene.work_id.clone();
    let chapter_id = scene.chapter_id.clone();
    let original_id = scene.id.clone();
    let position = scene.position;
    let result = blocking(&state.pool, move |conn| {
        let now = now_iso();
        conn.transaction::<_, ApiError, _>(|conn| {
            // Make room for the new scene without disturbing reading order.
            diesel::update(
                scenes::table
                    .filter(scenes::chapter_id.eq(&chapter_id))
                    .filter(scenes::position.gt(position)),
            )
            .set(scenes::position.eq(scenes::position + 1))
            .execute(conn)?;

            diesel::update(scenes::table.find(&original_id))
                .set((
                    scenes::content_hash.eq(doc::content_hash(&first_doc)),
                    scenes::word_count.eq(doc::word_count(&first_doc)),
                    scenes::updated_at.eq(&now),
                ))
                .execute(conn)?;

            diesel::insert_into(scenes::table)
                .values(&NewScene {
                    id: new_id_value.clone(),
                    work_id: work_id.clone(),
                    chapter_id: chapter_id.clone(),
                    title,
                    summary: None,
                    // Inherited: the second half of a scene is almost always in
                    // the same place, on the same day, in the same head.
                    pov: scene.pov.clone(),
                    status: scene.status.clone(),
                    position: position + 1,
                    word_count: doc::word_count(&second_doc),
                    content_hash: doc::content_hash(&second_doc),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    target_words: None,
                    location: scene.location.clone(),
                    story_date: scene.story_date.clone(),
                })
                .execute(conn)?;
            Ok(())
        })?;

        support::recount_work(conn, &work_id, &now_iso())?;
        Ok(SplitSceneResponse {
            first: scenes::table
                .find(&original_id)
                .select(Scene::as_select())
                .first(conn)?,
            second: scenes::table
                .find(&new_id_value)
                .select(Scene::as_select())
                .first(conn)?,
        })
    })
    .await?;

    Ok(Json(result))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MergeSceneRequest {
    /// The scene to absorb. It must sit in the same chapter.
    pub with: String,
    /// Both scenes' hashes, so a merge cannot silently swallow someone's edit.
    pub base_hash: String,
    pub with_base_hash: String,
}

/// Folds the next scene into this one and removes it.
///
/// The inverse of `split`: the absorbed scene's blocks follow this one's, in
/// order, and nothing is dropped at the seam.
#[utoipa::path(
    post,
    path = "/api/scenes/{scene_id}/merge",
    tag = "scenes",
    params(("scene_id" = String, Path)),
    request_body = MergeSceneRequest,
    responses(
        (status = 200, description = "The surviving scene", body = Scene),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn merge_scene(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Json(body): Json<MergeSceneRequest>,
) -> ApiResult<Json<Scene>> {
    if scene_id == body.with {
        return Err(ApiError::BadRequest(
            "a scene cannot be merged with itself".into(),
        ));
    }

    let lookup_user = user.clone();
    let lookup_id = scene_id.clone();
    let other_id = body.with.clone();
    let (keep, absorb) = blocking(&state.pool, move |conn| {
        let (keep, _) = support::scene_for(conn, &lookup_id, &lookup_user, Ability::Write)?;
        let (absorb, _) = support::scene_for(conn, &other_id, &lookup_user, Ability::Write)?;
        if keep.chapter_id != absorb.chapter_id {
            return Err(ApiError::BadRequest(
                "scenes can only be merged within one chapter — move it first".into(),
            ));
        }
        Ok((keep, absorb))
    })
    .await?;

    for (scene, expected) in [(&keep, &body.base_hash), (&absorb, &body.with_base_hash)] {
        if &scene.content_hash != expected {
            return Err(ApiError::ConflictWith {
                message: format!("`{}` changed since you chose to merge it", scene.title),
                details: serde_json::json!({
                    "sceneId": scene.id,
                    "currentHash": scene.content_hash,
                    "currentWordCount": scene.word_count,
                    "updatedAt": scene.updated_at,
                    "yourHash": expected,
                }),
            });
        }
    }

    let first = state
        .store
        .read_working_scene(&keep.work_id, &keep.id)
        .await?;
    let second = state
        .store
        .read_working_scene(&absorb.work_id, &absorb.id)
        .await?;
    // Order follows the page, not the argument order: whichever scene the
    // reader meets first keeps its text first.
    let merged = if keep.position <= absorb.position {
        doc::merge_docs(&first, &second)
    } else {
        doc::merge_docs(&second, &first)
    };

    state
        .store
        .write_working_scene(&keep.work_id, &keep.id, &merged)
        .await?;

    let work_id = keep.work_id.clone();
    let keep_id = keep.id.clone();
    let absorb_id = absorb.id.clone();
    let chapter_id = keep.chapter_id.clone();
    let absorbed_position = absorb.position;
    let scene = blocking(&state.pool, move |conn| {
        let now = now_iso();
        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::update(scenes::table.find(&keep_id))
                .set((
                    scenes::content_hash.eq(doc::content_hash(&merged)),
                    scenes::word_count.eq(doc::word_count(&merged)),
                    scenes::updated_at.eq(&now),
                ))
                .execute(conn)?;
            diesel::delete(scenes::table.find(&absorb_id)).execute(conn)?;
            // Close the gap the removed scene left.
            diesel::update(
                scenes::table
                    .filter(scenes::chapter_id.eq(&chapter_id))
                    .filter(scenes::position.gt(absorbed_position)),
            )
            .set(scenes::position.eq(scenes::position - 1))
            .execute(conn)?;
            Ok(())
        })?;
        support::recount_work(conn, &work_id, &now_iso())?;
        Ok(scenes::table
            .find(&keep_id)
            .select(Scene::as_select())
            .first(conn)?)
    })
    .await?;

    // The absorbed scene's file is no longer referenced by anything.
    state.store.delete_scene(&keep.work_id, &absorb.id).await?;

    Ok(Json(scene))
}
