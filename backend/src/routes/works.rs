use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{chapters, scenes, templates, works, writing_places};
use crate::domain::enums::{ActivityKind, AuditAction, Genre, WorkKind};
use crate::domain::templates::TemplateStructure;
use crate::domain::{doc, new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::collaborators::record;
use super::support::{self, Ability, Page};

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
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::Genre>)]
    pub genre: Option<String>,
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
    /// An empty string clears it.
    #[serde(default)]
    pub genre: Option<String>,
    /// How long the book is meant to be. `0` clears it.
    #[serde(default)]
    pub target_words: Option<i32>,
    /// `YYYY-MM-DD` to reach it by. An empty string clears it.
    #[serde(default)]
    pub target_date: Option<String>,
    /// Anything else sent. Kept only so the publishing details, which live on
    /// `/metadata`, are refused by name instead of silently dropped.
    #[serde(flatten)]
    #[schema(ignore)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// Fields of `/works/{id}/metadata` that a client might send here by mistake.
const METADATA_FIELDS: &[&str] = &[
    "language",
    "textDirection",
    "description",
    "publisher",
    "published",
    "identifier",
    "keywords",
    "contributors",
    "cover",
];

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutlineResponse {
    #[serde(flatten)]
    pub work: Work,
    pub chapters: Vec<OutlineChapter>,
    /// The caller's standing on the work, so a client can offer only what the
    /// server will accept. Sent on every outline; optional so an outline a
    /// client cached before collaboration existed still decodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<WorkAccess>,
    /// Where the caller was when they last had the book open, to put them
    /// back there. Absent the first time, or once that scene is gone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<WritingPlace>,
    /// Every status a scene in the book can have, the book's own first, with
    /// the colour each is shown in. Optional so an outline a client cached
    /// before a book could have its own still decodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statuses: Option<Vec<super::statuses::StatusOption>>,
    /// The book's reviews still open to readers, for a badge on what each
    /// covers. Sent to the people writing it; absent elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviews: Option<Vec<super::reviews::ReviewBadge>>,
}

/// Where a writer was in a book: the scene, and the selection in it, as the
/// editor's positions within that scene's document.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WritingPlace {
    pub scene_id: String,
    pub from: i32,
    pub to: i32,
}

/// What the outline tells a client about who it is to this work.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkAccess {
    #[schema(value_type = crate::domain::enums::CollaboratorRole)]
    pub role: String,
    /// The chapters they may change, when they were handed only some. Absent
    /// means the whole work. Reading is never narrowed.
    pub chapter_ids: Option<Vec<String>>,
    /// Whether they started the work — the one thing an added owner is not.
    pub creator: bool,
    /// Whose book it is.
    pub owner: support::AuthorCard,
}

/// The outline with the caller's standing attached.
pub(crate) fn outline_for(
    conn: &mut crate::db::Conn,
    work_id: &str,
    user: &AuthUser,
) -> ApiResult<OutlineResponse> {
    let (work, access) = support::work_access(conn, work_id, user)?;
    let owner = support::author_cards(conn, std::slice::from_ref(&work.owner_id))?
        .remove(&work.owner_id)
        .ok_or_else(|| ApiError::internal("a work without an owner"))?;
    let mut outline = load_outline(conn, work_id)?;
    outline.access = Some(WorkAccess {
        role: access.role,
        chapter_ids: access.chapters.map(|c| {
            let mut ids: Vec<String> = c.into_iter().collect();
            ids.sort();
            ids
        }),
        creator: access.creator,
        owner,
    });
    outline.place = writing_places::table
        .find((&user.id, work_id))
        .select((
            writing_places::scene_id,
            writing_places::sel_from,
            writing_places::sel_to,
        ))
        .first::<(String, i32, i32)>(conn)
        .optional()?
        .map(|(scene_id, from, to)| WritingPlace { scene_id, from, to });
    outline.reviews = Some(super::reviews::badges(conn, &outline)?);
    Ok(outline)
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutlineChapter {
    #[serde(flatten)]
    pub chapter: Chapter,
    pub scenes: Vec<Scene>,
}

fn check_genre(genre: Option<&str>) -> ApiResult<()> {
    match genre {
        Some(genre) if !Genre::accepts(genre) => Err(ApiError::BadRequest(format!(
            "genre must be one of {}",
            Genre::options()
        ))),
        _ => Ok(()),
    }
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
        use crate::db::schema::work_collaborators;
        // Shared work sits on the desk beside the writer's own; the outline
        // says what they may do with it.
        // Only once the address is proved — see `support::access`.
        let address = if user.email_verified {
            user.email.trim().to_lowercase()
        } else {
            String::new()
        };
        let shared = work_collaborators::table
            .filter(work_collaborators::email.eq(address))
            .select(work_collaborators::work_id);
        Ok(works::table
            .filter(works::owner_id.eq(&user.id).or(works::id.eq_any(shared)))
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
        support::work_for(conn, &work_id, &user, Ability::Read)
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
    check_genre(body.genre.as_deref())?;

    let work_id = new_id();
    let owner_id = user.id.clone();
    let db_work_id = work_id.clone();
    let creator = user.clone();

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
                    genre: body.genre.clone(),
                    target_words: None,
                    target_date: None,
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
                        role: "chapter".into(),
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

        let outline = outline_for(conn, &db_work_id, &creator)?;
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
    if let Some(field) = METADATA_FIELDS
        .iter()
        .find(|f| body.other.contains_key(**f))
    {
        return Err(ApiError::BadRequest(format!(
            "`{field}` is a publishing detail: send it to PATCH /api/works/{{id}}/metadata, \
             which saves every one of them"
        )));
    }
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
        if let Some(target) = body.target_words {
            if target < 0 {
                return Err(ApiError::BadRequest(
                    "a target is a number of words, or 0 for none".into(),
                ));
            }
            diesel::update(works::table.find(&work_id))
                .set(works::target_words.eq((target > 0).then_some(target)))
                .execute(conn)?;
        }
        if let Some(value) = text(&body.target_date) {
            if let Some(date) = &value {
                chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| {
                    ApiError::BadRequest("targetDate must look like YYYY-MM-DD".into())
                })?;
            }
            diesel::update(works::table.find(&work_id))
                .set(works::target_date.eq(value))
                .execute(conn)?;
        }
        if let Some(value) = text(&body.genre) {
            check_genre(value.as_deref())?;
            diesel::update(works::table.find(&work_id))
                .set(works::genre.eq(value))
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
        let (_, access) = support::work_access(conn, &owned, &user)?;
        // An owner someone added can do everything but this: the book is
        // still the person's who started it.
        if !access.creator {
            return Err(ApiError::Forbidden(
                "only the person who started this work can delete it".into(),
            ));
        }
        let title: String = works::table.find(&owned).select(works::title).first(conn)?;
        diesel::delete(works::table.find(&owned)).execute(conn)?;
        super::audit::record(
            conn,
            Some(&user.id),
            Some(&owned),
            AuditAction::WorkDeleted,
            &title,
        )?;
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
    let outline = blocking(&state.pool, move |conn| outline_for(conn, &work_id, &user)).await?;
    Ok(Json(outline))
}

/// Remember where the caller is in the book, so opening it again — here or on
/// another device — puts them back there. Anyone who can read the book has a
/// place in it; it is theirs alone and changes nothing for anyone else.
#[utoipa::path(
    put,
    path = "/api/works/{work_id}/place",
    tag = "works",
    params(("work_id" = String, Path)),
    request_body = WritingPlace,
    responses(
        (status = 200, description = "The place, as kept", body = WritingPlace),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn put_place(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(place): Json<WritingPlace>,
) -> ApiResult<Json<WritingPlace>> {
    if place.from < 0 || place.to < place.from {
        return Err(ApiError::BadRequest(
            "a place is a selection: from and to at or after the start, to not before from".into(),
        ));
    }
    let kept = place.clone();
    blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Read)?;
        let in_work = scenes::table
            .filter(scenes::id.eq(&place.scene_id))
            .filter(scenes::work_id.eq(&work_id))
            .count()
            .get_result::<i64>(conn)?
            > 0;
        if !in_work {
            return Err(ApiError::NotFound("that scene is not in this work".into()));
        }
        diesel::replace_into(writing_places::table)
            .values((
                writing_places::user_id.eq(&user.id),
                writing_places::work_id.eq(&work_id),
                writing_places::scene_id.eq(&place.scene_id),
                writing_places::sel_from.eq(place.from),
                writing_places::sel_to.eq(place.to),
                writing_places::updated_at.eq(now_iso()),
            ))
            .execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(kept))
}

/// The outline with every untitled chapter and scene called by where it
/// falls — "Chapter 3", "Scene 2" — as a version freezes them.
pub(crate) fn with_placeholder_names(mut outline: OutlineResponse) -> OutlineResponse {
    use crate::domain::naming::{chapter_names, scene_name_in};
    let names = chapter_names(
        outline
            .chapters
            .iter()
            .map(|c| (c.chapter.title.as_str(), c.chapter.role.as_str())),
    );
    for (chapter, name) in outline.chapters.iter_mut().zip(names) {
        chapter.chapter.title = name;
        for (s, scene) in chapter.scenes.iter_mut().enumerate() {
            scene.title = scene_name_in(&chapter.chapter.role, &scene.title, s + 1);
        }
    }
    outline
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
        access: None,
        place: None,
        statuses: Some(super::statuses::for_work(conn, work_id)?),
        reviews: None,
    })
}

// --- chapters ------------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateChapterRequest {
    /// Left out or empty, it is untitled and shown as "Chapter 2" for where it
    /// falls, until the writer names it.
    #[serde(default)]
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
    /// `chapter`, `front` (preface, foreword), `back` (acknowledgments,
    /// afterword, appendix) or `part` (a divider that opens a run of
    /// chapters). Front and back matter and parts are not numbered as
    /// chapters. A chapter made a part is given an empty scene if it has none:
    /// its part page, which keeps it in a version even with nothing on it.
    #[serde(default)]
    pub role: Option<String>,
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
        support::whole_work_for(conn, &work_id, &user, Ability::Write)?;
        let now = now_iso();
        let row = NewChapter {
            id: new_id(),
            work_id: work_id.clone(),
            title: body.title.trim().to_string(),
            summary: body.summary.clone(),
            position: support::next_chapter_position(conn, &work_id)?,
            created_at: now.clone(),
            updated_at: now,
            role: "chapter".into(),
        };
        diesel::insert_into(chapters::table)
            .values(&row)
            .execute(conn)?;
        let chapter: Chapter = chapters::table
            .find(&row.id)
            .select(Chapter::as_select())
            .first(conn)?;
        let label = support::chapter_label(conn, &chapter)?;
        record(
            conn,
            &row.work_id,
            &user.id,
            ActivityKind::ChapterAdded,
            Some(&row.id),
            &label,
        )?;
        Ok(chapter)
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
    let (chapter, part_page) = blocking(&state.pool, move |conn| {
        let (_, work) = support::chapter_for(conn, &chapter_id, &user, Ability::Write)?;
        let now = now_iso();
        let mut part_page = None;
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
        if let Some(role) = &body.role {
            if !matches!(role.as_str(), "chapter" | "front" | "back" | "part") {
                return Err(ApiError::BadRequest(
                    "role must be `chapter`, `front`, `back` or `part`".into(),
                ));
            }
            diesel::update(chapters::table.find(&chapter_id))
                .set(chapters::role.eq(role))
                .execute(conn)?;
            if role == "part" {
                part_page = ensure_part_page(conn, &work.id, &chapter_id, &now)?;
            }
        }
        diesel::update(chapters::table.find(&chapter_id))
            .set(chapters::updated_at.eq(&now))
            .execute(conn)?;
        let chapter = chapters::table
            .find(&chapter_id)
            .select(Chapter::as_select())
            .first(conn)?;
        Ok((chapter, part_page))
    })
    .await?;
    if let Some(scene_id) = part_page {
        state
            .store
            .write_working_scene(&chapter.work_id, &scene_id, &doc::empty_doc())
            .await?;
    }
    Ok(Json(chapter))
}

/// A part with no scene gets an empty one, its part page: a version keeps a
/// chapter only by its scenes, and a part should not vanish from one for
/// having nothing on its page. The id of the scene made, if one was.
fn ensure_part_page(
    conn: &mut crate::db::Conn,
    work_id: &str,
    chapter_id: &str,
    now: &str,
) -> ApiResult<Option<String>> {
    let has_scene: Option<String> = scenes::table
        .filter(scenes::chapter_id.eq(chapter_id))
        .select(scenes::id)
        .first(conn)
        .optional()?;
    if has_scene.is_some() {
        return Ok(None);
    }
    let empty = doc::empty_doc();
    let id = new_id();
    diesel::insert_into(scenes::table)
        .values(&NewScene {
            id: id.clone(),
            work_id: work_id.to_string(),
            chapter_id: chapter_id.to_string(),
            title: String::new(),
            summary: None,
            pov: None,
            status: "drafting".into(),
            position: 0,
            word_count: 0,
            content_hash: doc::content_hash(&empty),
            created_at: now.to_string(),
            updated_at: now.to_string(),
            target_words: None,
            location: None,
            story_date: None,
        })
        .execute(conn)?;
    Ok(Some(id))
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
        let (chapter, work) = support::chapter_for(conn, &chapter_id, &user, Ability::Write)?;
        // Removing a chapter changes the shape of the book, which someone
        // handed that chapter alone does not get to do.
        support::whole_work_for(conn, &work.id, &user, Ability::Write)?;
        let ids: Vec<String> = scenes::table
            .filter(scenes::chapter_id.eq(&chapter.id))
            .select(scenes::id)
            .load(conn)?;
        // Named before it goes, while it still has a place to be named by.
        let label = support::chapter_label(conn, &chapter)?;
        diesel::delete(chapters::table.find(&chapter.id)).execute(conn)?;
        support::recount_work(conn, &work.id, &now_iso())?;
        record(
            conn,
            &work.id,
            &user.id,
            ActivityKind::ChapterRemoved,
            Some(&chapter.id),
            &label,
        )?;
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
        support::whole_work_for(conn, &work_id, &user, Ability::Write)?;
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
