use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{chapters, scenes, templates, works};
use crate::domain::enums::WorkKind;
use crate::domain::templates::TemplateStructure;
use crate::domain::{doc, new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, Page};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateWorkRequest {
    pub title: String,
    #[schema(value_type = crate::domain::enums::WorkKind)]
    pub kind: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub synopsis: Option<String>,
    /// Omit for a single empty chapter and scene.
    #[serde(default)]
    pub template_id: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateWorkRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub synopsis: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::WorkStatus>)]
    pub status: Option<String>,
    /// Front matter. Each is sent only when it changes; an empty string clears
    /// one, which is what a writer emptying the field in the UI means.
    #[serde(default)]
    pub pen_name: Option<String>,
    #[serde(default)]
    pub short_title: Option<String>,
    #[serde(default)]
    pub dedication: Option<String>,
    #[serde(default)]
    pub epigraph: Option<String>,
    #[serde(default)]
    pub epigraph_source: Option<String>,
    #[serde(default)]
    pub copyright_line: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutlineResponse {
    #[serde(flatten)]
    pub work: Work,
    pub chapters: Vec<OutlineChapter>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutlineChapter {
    #[serde(flatten)]
    pub chapter: Chapter,
    pub scenes: Vec<Scene>,
}

/// Scene bodies produced while materialising a template, written to disk after
/// the transaction commits.
struct PendingScene {
    scene_id: String,
    body: Value,
}

#[utoipa::path(
    get,
    path = "/api/works",
    tag = "works",
    responses(
        (status = 200, description = "Success", body = Vec<Work>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(page): Query<Page>,
) -> ApiResult<Json<Vec<Work>>> {
    let (limit, offset) = page.clamped();
    let rows = blocking(&state.pool, move |conn| {
        Ok(works::table
            .filter(works::owner_id.eq(&user.id))
            .order(works::updated_at.desc())
            .limit(limit)
            .offset(offset)
            .select(Work::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(Json(rows))
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}",
    tag = "works",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = Work),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Work>> {
    let work = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &work_id, &user)
    })
    .await?;
    Ok(Json(work))
}

#[utoipa::path(
    post,
    path = "/api/works",
    tag = "works",
    request_body = CreateWorkRequest,
    responses(
        (status = 200, description = "Success", body = OutlineResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateWorkRequest>,
) -> ApiResult<Json<OutlineResponse>> {
    let title = body.title.trim().to_string();
    if title.is_empty() {
        return Err(ApiError::BadRequest("title is required".into()));
    }
    if !WorkKind::accepts(&body.kind) {
        return Err(ApiError::BadRequest(format!(
            "kind must be one of {}",
            WorkKind::options()
        )));
    }

    let work_id = new_id();
    let owner_id = user.id.clone();
    let db_work_id = work_id.clone();

    let (outline, pending) = blocking(&state.pool, move |conn| {
        let structure = match &body.template_id {
            Some(template_id) => {
                let raw: String = templates::table
                    .find(template_id)
                    .select(templates::structure_json)
                    .first(conn)
                    .optional()?
                    .ok_or_else(|| ApiError::NotFound("template not found".into()))?;
                serde_json::from_str::<TemplateStructure>(&raw)?
            }
            None => TemplateStructure {
                chapters: vec![crate::domain::templates::TemplateChapter {
                    title: "Chapter One".into(),
                    summary: None,
                    scenes: vec![crate::domain::templates::TemplateScene {
                        title: "Scene 1".into(),
                        summary: None,
                        body: None,
                    }],
                }],
            },
        };

        let now = now_iso();
        let mut pending: Vec<PendingScene> = Vec::new();

        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::insert_into(works::table)
                .values(&NewWork {
                    id: db_work_id.clone(),
                    owner_id: owner_id.clone(),
                    title: title.clone(),
                    subtitle: body.subtitle.clone(),
                    kind: body.kind.clone(),
                    synopsis: body.synopsis.clone(),
                    template_id: body.template_id.clone(),
                    status: "draft".into(),
                    current_version_id: None,
                    word_count: 0,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    pen_name: None,
                    short_title: None,
                    dedication: None,
                    epigraph: None,
                    epigraph_source: None,
                    copyright_line: None,
                    pinned_version_id: None,
                })
                .execute(conn)?;

            for (chapter_index, chapter_spec) in structure.chapters.iter().enumerate() {
                let chapter_id = new_id();
                diesel::insert_into(chapters::table)
                    .values(&NewChapter {
                        id: chapter_id.clone(),
                        work_id: db_work_id.clone(),
                        title: chapter_spec.title.clone(),
                        summary: chapter_spec.summary.clone(),
                        position: chapter_index as i32,
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    })
                    .execute(conn)?;

                for (scene_index, scene_spec) in chapter_spec.scenes.iter().enumerate() {
                    let scene_id = new_id();
                    let body_doc = match scene_spec.body.as_deref() {
                        Some(text) if !text.trim().is_empty() => doc::doc_from_text(text),
                        _ => doc::empty_doc(),
                    };
                    diesel::insert_into(scenes::table)
                        .values(&NewScene {
                            id: scene_id.clone(),
                            work_id: db_work_id.clone(),
                            chapter_id: chapter_id.clone(),
                            title: scene_spec.title.clone(),
                            summary: scene_spec.summary.clone(),
                            pov: None,
                            status: "outline".into(),
                            position: scene_index as i32,
                            word_count: doc::word_count(&body_doc),
                            content_hash: doc::content_hash(&body_doc),
                            created_at: now.clone(),
                            updated_at: now.clone(),
                            target_words: None,
                            location: None,
                            story_date: None,
                        })
                        .execute(conn)?;
                    pending.push(PendingScene {
                        scene_id,
                        body: body_doc,
                    });
                }
            }

            support::recount_work(conn, &db_work_id, &now)?;
            Ok(())
        })?;

        let outline = load_outline(conn, &db_work_id)?;
        Ok((outline, pending))
    })
    .await?;

    for scene in pending {
        state
            .store
            .write_working_scene(&work_id, &scene.scene_id, &scene.body)
            .await?;
    }

    Ok(Json(outline))
}

#[utoipa::path(
    patch,
    path = "/api/works/{work_id}",
    tag = "works",
    params(("work_id" = String, Path)),
    request_body = UpdateWorkRequest,
    responses(
        (status = 200, description = "Success", body = Work),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<UpdateWorkRequest>,
) -> ApiResult<Json<Work>> {
    let work = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &work_id, &user)?;
        let now = now_iso();
        if let Some(title) = &body.title {
            if title.trim().is_empty() {
                return Err(ApiError::BadRequest("title cannot be empty".into()));
            }
            diesel::update(works::table.find(&work_id))
                .set(works::title.eq(title.trim()))
                .execute(conn)?;
        }
        if let Some(subtitle) = &body.subtitle {
            diesel::update(works::table.find(&work_id))
                .set(works::subtitle.eq(subtitle))
                .execute(conn)?;
        }
        if let Some(synopsis) = &body.synopsis {
            diesel::update(works::table.find(&work_id))
                .set(works::synopsis.eq(synopsis))
                .execute(conn)?;
        }
        if let Some(status) = &body.status {
            // The lifecycle state of a work is derived from its versions, except
            // for archiving, which is a deliberate author action.
            if status != "archived" && status != "draft" {
                return Err(ApiError::BadRequest(
                    "status may only be set to `draft` or `archived`; publish a version instead"
                        .into(),
                ));
            }
            diesel::update(works::table.find(&work_id))
                .set(works::status.eq(status))
                .execute(conn)?;
        }
        // Blank means "clear it", so a writer can take a pen name off again.
        let text = |value: &Option<String>| -> Option<Option<String>> {
            value.as_ref().map(|v| {
                let trimmed = v.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
        };
        if let Some(value) = text(&body.pen_name) {
            diesel::update(works::table.find(&work_id))
                .set(works::pen_name.eq(value))
                .execute(conn)?;
        }
        if let Some(value) = text(&body.short_title) {
            diesel::update(works::table.find(&work_id))
                .set(works::short_title.eq(value))
                .execute(conn)?;
        }
        if let Some(value) = text(&body.dedication) {
            diesel::update(works::table.find(&work_id))
                .set(works::dedication.eq(value))
                .execute(conn)?;
        }
        if let Some(value) = text(&body.epigraph) {
            diesel::update(works::table.find(&work_id))
                .set(works::epigraph.eq(value))
                .execute(conn)?;
        }
        if let Some(value) = text(&body.epigraph_source) {
            diesel::update(works::table.find(&work_id))
                .set(works::epigraph_source.eq(value))
                .execute(conn)?;
        }
        if let Some(value) = text(&body.copyright_line) {
            diesel::update(works::table.find(&work_id))
                .set(works::copyright_line.eq(value))
                .execute(conn)?;
        }
        diesel::update(works::table.find(&work_id))
            .set(works::updated_at.eq(&now))
            .execute(conn)?;
        Ok(works::table
            .find(&work_id)
            .select(Work::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(work))
}

#[utoipa::path(
    delete,
    path = "/api/works/{work_id}",
    tag = "works",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "Success"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_work(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let owned = work_id.clone();
    blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &owned, &user)?;
        diesel::delete(works::table.find(&owned)).execute(conn)?;
        Ok(())
    })
    .await?;
    state.store.delete_work(&work_id).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/outline",
    tag = "works",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = OutlineResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn outline(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<Json<OutlineResponse>> {
    let outline = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &work_id, &user)?;
        load_outline(conn, &work_id)
    })
    .await?;
    Ok(Json(outline))
}

pub(crate) fn load_outline(
    conn: &mut crate::db::Conn,
    work_id: &str,
) -> ApiResult<OutlineResponse> {
    let work: Work = works::table
        .find(work_id)
        .select(Work::as_select())
        .first(conn)?;
    let chapter_rows: Vec<Chapter> = chapters::table
        .filter(chapters::work_id.eq(work_id))
        .order(chapters::position.asc())
        .select(Chapter::as_select())
        .load(conn)?;
    let scene_rows: Vec<Scene> = scenes::table
        .filter(scenes::work_id.eq(work_id))
        .order((scenes::chapter_id.asc(), scenes::position.asc()))
        .select(Scene::as_select())
        .load(conn)?;

    let chapters_out = chapter_rows
        .into_iter()
        .map(|chapter| {
            let scenes = scene_rows
                .iter()
                .filter(|s| s.chapter_id == chapter.id)
                .cloned()
                .collect();
            OutlineChapter { chapter, scenes }
        })
        .collect();

    Ok(OutlineResponse {
        work,
        chapters: chapters_out,
    })
}

// --- chapters ------------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateChapterRequest {
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateChapterRequest {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReorderRequest {
    /// Complete ordered list of ids; anything missing keeps its current slot.
    pub ids: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/chapters",
    tag = "chapters",
    params(("work_id" = String, Path)),
    request_body = CreateChapterRequest,
    responses(
        (status = 200, description = "Success", body = Chapter),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create_chapter(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<CreateChapterRequest>,
) -> ApiResult<Json<Chapter>> {
    let chapter = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &work_id, &user)?;
        let now = now_iso();
        let row = NewChapter {
            id: new_id(),
            work_id: work_id.clone(),
            title: body.title.trim().to_string(),
            summary: body.summary.clone(),
            position: support::next_chapter_position(conn, &work_id)?,
            created_at: now.clone(),
            updated_at: now,
        };
        diesel::insert_into(chapters::table)
            .values(&row)
            .execute(conn)?;
        Ok(chapters::table
            .find(&row.id)
            .select(Chapter::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(chapter))
}

#[utoipa::path(
    patch,
    path = "/api/chapters/{chapter_id}",
    tag = "chapters",
    params(("chapter_id" = String, Path)),
    request_body = UpdateChapterRequest,
    responses(
        (status = 200, description = "Success", body = Chapter),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update_chapter(
    State(state): State<AppState>,
    user: AuthUser,
    Path(chapter_id): Path<String>,
    Json(body): Json<UpdateChapterRequest>,
) -> ApiResult<Json<Chapter>> {
    let chapter = blocking(&state.pool, move |conn| {
        support::chapter_for_owner(conn, &chapter_id, &user)?;
        let now = now_iso();
        if let Some(title) = &body.title {
            diesel::update(chapters::table.find(&chapter_id))
                .set(chapters::title.eq(title.trim()))
                .execute(conn)?;
        }
        if let Some(summary) = &body.summary {
            diesel::update(chapters::table.find(&chapter_id))
                .set(chapters::summary.eq(summary))
                .execute(conn)?;
        }
        diesel::update(chapters::table.find(&chapter_id))
            .set(chapters::updated_at.eq(&now))
            .execute(conn)?;
        Ok(chapters::table
            .find(&chapter_id)
            .select(Chapter::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(chapter))
}

#[utoipa::path(
    delete,
    path = "/api/chapters/{chapter_id}",
    tag = "chapters",
    params(("chapter_id" = String, Path)),
    responses(
        (status = 200, description = "Success"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_chapter(
    State(state): State<AppState>,
    user: AuthUser,
    Path(chapter_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let scene_ids = blocking(&state.pool, move |conn| {
        let (chapter, work) = support::chapter_for_owner(conn, &chapter_id, &user)?;
        let ids: Vec<String> = scenes::table
            .filter(scenes::chapter_id.eq(&chapter.id))
            .select(scenes::id)
            .load(conn)?;
        diesel::delete(chapters::table.find(&chapter.id)).execute(conn)?;
        support::recount_work(conn, &work.id, &now_iso())?;
        Ok((work.id, ids))
    })
    .await?;
    let (work_id, ids) = scene_ids;
    for scene_id in ids {
        state.store.delete_scene(&work_id, &scene_id).await?;
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/chapters/reorder",
    tag = "chapters",
    params(("work_id" = String, Path)),
    request_body = ReorderRequest,
    responses(
        (status = 200, description = "Success", body = Vec<Chapter>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn reorder_chapters(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<ReorderRequest>,
) -> ApiResult<Json<Vec<Chapter>>> {
    let rows = blocking(&state.pool, move |conn| {
        support::work_for_owner(conn, &work_id, &user)?;
        let now = now_iso();
        conn.transaction::<_, ApiError, _>(|conn| {
            for (index, id) in body.ids.iter().enumerate() {
                diesel::update(
                    chapters::table
                        .find(id)
                        .filter(chapters::work_id.eq(&work_id)),
                )
                .set((
                    chapters::position.eq(index as i32),
                    chapters::updated_at.eq(&now),
                ))
                .execute(conn)?;
            }
            Ok(())
        })?;
        Ok(chapters::table
            .filter(chapters::work_id.eq(&work_id))
            .order(chapters::position.asc())
            .select(Chapter::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(Json(rows))
}
