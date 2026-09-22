use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{scene_revisions, scenes};
use crate::domain::{doc, new_id, now_iso};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

use super::support::{self, Page};

const SCENE_STATUSES: &[&str] = &["outline", "drafting", "revised", "done"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSceneRequest {
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub pov: Option<String>,
    #[serde(default)]
    pub content: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSceneRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub pov: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    /// Move the scene into a different chapter of the same work.
    #[serde(default)]
    pub chapter_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PutContentRequest {
    pub content: Value,
    /// Autosaves pass `false`; explicit saves and idle-flushes pass `true`.
    #[serde(default = "default_true")]
    pub create_revision: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneWithContent {
    #[serde(flatten)]
    pub scene: Scene,
    pub content: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveResult {
    pub scene: Scene,
    pub revision_id: Option<String>,
    pub work_word_count: i32,
}

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
        let (chapter, work) = support::chapter_for_owner(conn, &chapter_id, &user)?;
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
        };
        diesel::insert_into(scenes::table)
            .values(&row)
            .execute(conn)?;
        support::recount_work(conn, &work.id, &now)?;
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

pub async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
) -> ApiResult<Json<SceneWithContent>> {
    let scene = blocking(&state.pool, move |conn| {
        let (scene, _) = support::scene_for_owner(conn, &scene_id, &user)?;
        Ok(scene)
    })
    .await?;
    let content = state
        .store
        .read_working_scene(&scene.work_id, &scene.id)
        .await?;
    Ok(Json(SceneWithContent { scene, content }))
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Json(body): Json<UpdateSceneRequest>,
) -> ApiResult<Json<Scene>> {
    if let Some(status) = &body.status {
        if !SCENE_STATUSES.contains(&status.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "status must be one of {}",
                SCENE_STATUSES.join(", ")
            )));
        }
    }
    let scene = blocking(&state.pool, move |conn| {
        let (scene, work) = support::scene_for_owner(conn, &scene_id, &user)?;
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
        if let Some(status) = &body.status {
            diesel::update(scenes::table.find(&scene.id))
                .set(scenes::status.eq(status))
                .execute(conn)?;
        }
        if let Some(target_chapter) = &body.chapter_id {
            let (chapter, _) = support::chapter_for_owner(conn, target_chapter, &user)?;
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
        let (scene, _) = support::scene_for_owner(conn, &lookup_id, &lookup_user)?;
        Ok(scene)
    })
    .await?;

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
    let scene_id_db = scene.id.clone();
    let revision_for_db = revision_id.clone();
    let author_id = user.id.clone();
    let (scene, work_word_count) = blocking(&state.pool, move |conn| {
        let now = now_iso();
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
        let scene: Scene = scenes::table
            .find(&scene_id_db)
            .select(Scene::as_select())
            .first(conn)?;
        Ok((scene, total))
    })
    .await?;

    Ok(Json(SaveResult {
        scene,
        revision_id,
        work_word_count,
    }))
}

pub async fn delete_scene(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (work_id, scene_id) = blocking(&state.pool, move |conn| {
        let (scene, work) = support::scene_for_owner(conn, &scene_id, &user)?;
        diesel::delete(scenes::table.find(&scene.id)).execute(conn)?;
        support::recount_work(conn, &work.id, &now_iso())?;
        Ok((work.id, scene.id))
    })
    .await?;
    state.store.delete_scene(&work_id, &scene_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn reorder(
    State(state): State<AppState>,
    user: AuthUser,
    Path(chapter_id): Path<String>,
    Json(body): Json<super::works::ReorderRequest>,
) -> ApiResult<Json<Vec<Scene>>> {
    let rows = blocking(&state.pool, move |conn| {
        let (chapter, _) = support::chapter_for_owner(conn, &chapter_id, &user)?;
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

pub async fn list_revisions(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Query(page): Query<Page>,
) -> ApiResult<Json<Vec<SceneRevision>>> {
    let (limit, offset) = page.clamped();
    let rows = blocking(&state.pool, move |conn| {
        let (scene, _) = support::scene_for_owner(conn, &scene_id, &user)?;
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionWithContent {
    #[serde(flatten)]
    pub revision: SceneRevision,
    pub content: Value,
    /// Word diff against the scene as it stands now.
    pub changes: Vec<crate::domain::diff::TextChange>,
}

pub async fn get_revision(
    State(state): State<AppState>,
    user: AuthUser,
    Path((scene_id, revision_id)): Path<(String, String)>,
) -> ApiResult<Json<RevisionWithContent>> {
    let revision_lookup = revision_id.clone();
    let (scene, revision) = blocking(&state.pool, move |conn| {
        let (scene, _) = support::scene_for_owner(conn, &scene_id, &user)?;
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
    let changes =
        crate::domain::diff::diff_text(&doc::plain_text(&content), &doc::plain_text(&current));

    Ok(Json(RevisionWithContent {
        revision,
        content,
        changes,
    }))
}
