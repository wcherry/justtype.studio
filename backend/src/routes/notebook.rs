//! The notebook: characters, places, and the continuity notes that belong to
//! neither.
//!
//! Entries link to scenes through a join table rather than a list on either
//! side, because the question gets asked from both directions — "who is in this
//! scene" while writing it, and "which scenes is she in" while fixing her eye
//! colour in chapter nine.
use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::{NewNotebookEntry, NewSceneNotebookLink, NotebookEntry};
use crate::db::schema::{notebook_entries, scene_notebook_links, scenes};
use crate::domain::enums::NotebookKind;
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, Ability};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEntryRequest {
    /// `character`, `place` or `note`.
    #[schema(value_type = crate::domain::enums::NotebookKind)]
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    /// A sketch template (`GET /api/notebook/templates`) to start from: its fields
    /// arrive empty, ready to fill in.
    #[serde(default)]
    pub template_id: Option<String>,
}

/// One named part of a sketch.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SketchField {
    /// Stable within the entry; a field the writer added gets one too.
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub value: String,
    /// The question the field asks, from its template. Absent on the writer's
    /// own fields.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
}

const MAX_FIELDS: usize = 40;
const MAX_FIELD_CHARS: usize = 20_000;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEntryRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    /// The whole sketch, in order. Replaces what was there.
    #[serde(default)]
    pub fields: Option<Vec<SketchField>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryQuery {
    /// Narrow to one kind.
    #[serde(default)]
    pub kind: Option<String>,
    /// Only the entries appearing in this scene.
    #[serde(default)]
    pub scene_id: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    #[serde(flatten)]
    pub entry: NotebookEntry,
    /// Scenes this entry appears in, in reading order.
    pub scene_ids: Vec<String>,
    /// The sketch, in order. Empty for an entry that is just a name and notes.
    pub fields: Vec<SketchField>,
}

fn parse_fields(raw: Option<&str>) -> Vec<SketchField> {
    raw.and_then(|r| serde_json::from_str(r).ok())
        .unwrap_or_default()
}

fn checked_fields(mut fields: Vec<SketchField>) -> ApiResult<String> {
    if fields.len() > MAX_FIELDS {
        return Err(ApiError::BadRequest(format!(
            "a sketch has at most {MAX_FIELDS} fields"
        )));
    }
    let mut seen = std::collections::HashSet::new();
    for field in &mut fields {
        field.label = field.label.trim().to_string();
        if field.label.is_empty() {
            return Err(ApiError::BadRequest("every field needs a label".into()));
        }
        if field.key.trim().is_empty() {
            field.key = new_id();
        }
        if !seen.insert(field.key.clone()) {
            return Err(ApiError::BadRequest(format!(
                "two fields share the key `{}`",
                field.key
            )));
        }
        if field.value.chars().count() > MAX_FIELD_CHARS {
            return Err(ApiError::BadRequest(format!(
                "`{}` is longer than {MAX_FIELD_CHARS} characters",
                field.label
            )));
        }
    }
    Ok(serde_json::to_string(&fields)?)
}

/// The sketch templates entries can be started from.
#[utoipa::path(
    get,
    path = "/api/notebook/templates",
    tag = "notebook",
    responses((status = 200, description = "Built-in sketch templates", body = Vec<crate::domain::sketches::SketchTemplate>))
)]
pub async fn templates() -> Json<Vec<crate::domain::sketches::SketchTemplate>> {
    Json(crate::domain::sketches::all())
}

fn load_entry(conn: &mut crate::db::Conn, entry_id: &str) -> ApiResult<EntryView> {
    let entry: NotebookEntry = notebook_entries::table
        .find(entry_id)
        .select(NotebookEntry::as_select())
        .first(conn)?;
    let scene_ids = scene_notebook_links::table
        .inner_join(scenes::table.on(scenes::id.eq(scene_notebook_links::scene_id)))
        .filter(scene_notebook_links::entry_id.eq(entry_id))
        .order((scenes::chapter_id.asc(), scenes::position.asc()))
        .select(scene_notebook_links::scene_id)
        .load(conn)?;
    Ok(EntryView {
        fields: parse_fields(entry.fields.as_deref()),
        entry,
        scene_ids,
    })
}

/// The caller owns the work this entry belongs to.
fn entry_for(
    conn: &mut crate::db::Conn,
    entry_id: &str,
    user: &AuthUser,
    need: Ability,
) -> ApiResult<NotebookEntry> {
    let entry: NotebookEntry = notebook_entries::table
        .find(entry_id)
        .select(NotebookEntry::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("entry not found".into()))?;
    support::work_for(conn, &entry.work_id, user, need)?;
    Ok(entry)
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/notebook",
    tag = "notebook",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "The work's entries", body = Vec<EntryView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Query(query): Query<EntryQuery>,
) -> ApiResult<Json<Vec<EntryView>>> {
    let entries = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Read)?;

        let ids: Vec<String> = match &query.scene_id {
            Some(scene_id) => scene_notebook_links::table
                .filter(scene_notebook_links::scene_id.eq(scene_id))
                .select(scene_notebook_links::entry_id)
                .load(conn)?,
            None => {
                let mut q = notebook_entries::table
                    .filter(notebook_entries::work_id.eq(&work_id))
                    .into_boxed();
                if let Some(kind) = &query.kind {
                    q = q.filter(notebook_entries::kind.eq(kind.clone()));
                }
                q.order((
                    notebook_entries::kind.asc(),
                    notebook_entries::position.asc(),
                    notebook_entries::name.asc(),
                ))
                .select(notebook_entries::id)
                .load(conn)?
            }
        };

        ids.iter().map(|id| load_entry(conn, id)).collect()
    })
    .await?;
    Ok(Json(entries))
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/notebook",
    tag = "notebook",
    params(("work_id" = String, Path)),
    request_body = CreateEntryRequest,
    responses(
        (status = 200, description = "The new entry", body = EntryView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<CreateEntryRequest>,
) -> ApiResult<Json<EntryView>> {
    if !NotebookKind::accepts(&body.kind) {
        return Err(ApiError::BadRequest(format!(
            "kind must be one of {}",
            NotebookKind::options()
        )));
    }
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::BadRequest("an entry needs a name".into()));
    }

    let template = match &body.template_id {
        Some(id) => {
            let template = crate::domain::sketches::find(id)
                .ok_or_else(|| ApiError::BadRequest(format!("no sketch template `{id}`")))?;
            if template.kind != body.kind {
                return Err(ApiError::BadRequest(format!(
                    "`{}` is a template for a {}, not a {}",
                    template.name, template.kind, body.kind
                )));
            }
            Some(template)
        }
        None => None,
    };
    let fields = template.as_ref().map(|t| {
        t.fields
            .iter()
            .map(|f| SketchField {
                key: f.key.into(),
                label: f.label.into(),
                value: String::new(),
                prompt: Some(f.prompt.into()),
            })
            .collect::<Vec<_>>()
    });
    let fields = match fields {
        Some(fields) => Some(checked_fields(fields)?),
        None => None,
    };

    let entry = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Write)?;
        let now = now_iso();
        // A new entry goes at the end of its kind.
        let last: Option<i32> = notebook_entries::table
            .filter(notebook_entries::work_id.eq(&work_id))
            .filter(notebook_entries::kind.eq(&body.kind))
            .select(diesel::dsl::max(notebook_entries::position))
            .first(conn)?;
        let row = NewNotebookEntry {
            id: new_id(),
            work_id,
            kind: body.kind.clone(),
            name,
            summary: body.summary.clone(),
            body: body.body.clone(),
            created_at: now.clone(),
            updated_at: now,
            fields,
            template_id: template.map(|t| t.id.to_string()),
            position: last.map_or(0, |p| p + 1),
        };
        diesel::insert_into(notebook_entries::table)
            .values(&row)
            .execute(conn)?;
        load_entry(conn, &row.id)
    })
    .await?;
    Ok(Json(entry))
}

#[utoipa::path(
    patch,
    path = "/api/notebook/{entry_id}",
    tag = "notebook",
    params(("entry_id" = String, Path)),
    request_body = UpdateEntryRequest,
    responses(
        (status = 200, description = "The updated entry", body = EntryView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(entry_id): Path<String>,
    Json(body): Json<UpdateEntryRequest>,
) -> ApiResult<Json<EntryView>> {
    let entry = blocking(&state.pool, move |conn| {
        entry_for(conn, &entry_id, &user, Ability::Write)?;
        if let Some(name) = &body.name {
            if name.trim().is_empty() {
                return Err(ApiError::BadRequest("an entry needs a name".into()));
            }
            diesel::update(notebook_entries::table.find(&entry_id))
                .set(notebook_entries::name.eq(name.trim()))
                .execute(conn)?;
        }
        if let Some(summary) = &body.summary {
            diesel::update(notebook_entries::table.find(&entry_id))
                .set(notebook_entries::summary.eq(summary))
                .execute(conn)?;
        }
        if let Some(text) = &body.body {
            diesel::update(notebook_entries::table.find(&entry_id))
                .set(notebook_entries::body.eq(text))
                .execute(conn)?;
        }
        if let Some(fields) = body.fields.clone() {
            diesel::update(notebook_entries::table.find(&entry_id))
                .set(notebook_entries::fields.eq(checked_fields(fields)?))
                .execute(conn)?;
        }
        diesel::update(notebook_entries::table.find(&entry_id))
            .set(notebook_entries::updated_at.eq(now_iso()))
            .execute(conn)?;
        load_entry(conn, &entry_id)
    })
    .await?;
    Ok(Json(entry))
}

#[utoipa::path(
    delete,
    path = "/api/notebook/{entry_id}",
    tag = "notebook",
    params(("entry_id" = String, Path)),
    responses(
        (status = 200, description = "Removed"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_entry(
    State(state): State<AppState>,
    user: AuthUser,
    Path(entry_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    blocking(&state.pool, move |conn| {
        entry_for(conn, &entry_id, &user, Ability::Write)?;
        diesel::delete(notebook_entries::table.find(&entry_id)).execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReorderEntriesRequest {
    /// The kind being arranged: entries are ordered within their kind.
    #[schema(value_type = crate::domain::enums::NotebookKind)]
    pub kind: String,
    /// The entries of that kind, in their new order. Any left out keep their
    /// order among themselves, after the ones listed.
    pub ids: Vec<String>,
}

/// Puts one kind of entry in the writer's order, as a drag in the notebook
/// leaves it.
#[utoipa::path(
    post,
    path = "/api/works/{work_id}/notebook/reorder",
    tag = "notebook",
    params(("work_id" = String, Path)),
    request_body = ReorderEntriesRequest,
    responses(
        (status = 200, description = "That kind's entries, in their new order", body = Vec<EntryView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn reorder(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<ReorderEntriesRequest>,
) -> ApiResult<Json<Vec<EntryView>>> {
    if !NotebookKind::accepts(&body.kind) {
        return Err(ApiError::BadRequest(format!(
            "kind must be one of {}",
            NotebookKind::options()
        )));
    }
    let entries = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Write)?;
        let current: Vec<String> = notebook_entries::table
            .filter(notebook_entries::work_id.eq(&work_id))
            .filter(notebook_entries::kind.eq(&body.kind))
            .order((
                notebook_entries::position.asc(),
                notebook_entries::name.asc(),
            ))
            .select(notebook_entries::id)
            .load(conn)?;
        if let Some(stray) = body.ids.iter().find(|id| !current.contains(id)) {
            return Err(ApiError::BadRequest(format!(
                "`{stray}` is not one of this book's {} entries",
                body.kind
            )));
        }
        let mut order: Vec<&String> = Vec::with_capacity(current.len());
        for id in body.ids.iter().chain(current.iter()) {
            if !order.contains(&id) {
                order.push(id);
            }
        }
        conn.transaction::<_, ApiError, _>(|conn| {
            for (index, id) in order.iter().enumerate() {
                diesel::update(notebook_entries::table.find(*id))
                    .set(notebook_entries::position.eq(index as i32))
                    .execute(conn)?;
            }
            Ok(())
        })?;
        order.iter().map(|id| load_entry(conn, id)).collect()
    })
    .await?;
    Ok(Json(entries))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkRequest {
    pub scene_id: String,
}

/// Records that an entry appears in a scene. Idempotent — linking twice is what
/// a writer does when they are not sure whether they already did.
#[utoipa::path(
    post,
    path = "/api/notebook/{entry_id}/links",
    tag = "notebook",
    params(("entry_id" = String, Path)),
    request_body = LinkRequest,
    responses(
        (status = 200, description = "The entry, with its scenes", body = EntryView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn link(
    State(state): State<AppState>,
    user: AuthUser,
    Path(entry_id): Path<String>,
    Json(body): Json<LinkRequest>,
) -> ApiResult<Json<EntryView>> {
    let entry = blocking(&state.pool, move |conn| {
        let entry = entry_for(conn, &entry_id, &user, Ability::Write)?;
        let (scene, _) = support::scene_for(conn, &body.scene_id, &user, Ability::Read)?;
        if scene.work_id != entry.work_id {
            return Err(ApiError::BadRequest(
                "an entry can only be linked to scenes in its own work".into(),
            ));
        }
        diesel::insert_into(scene_notebook_links::table)
            .values(&NewSceneNotebookLink {
                id: new_id(),
                scene_id: scene.id,
                entry_id: entry_id.clone(),
                created_at: now_iso(),
            })
            .on_conflict((
                scene_notebook_links::scene_id,
                scene_notebook_links::entry_id,
            ))
            .do_nothing()
            .execute(conn)?;
        load_entry(conn, &entry_id)
    })
    .await?;
    Ok(Json(entry))
}

#[utoipa::path(
    delete,
    path = "/api/notebook/{entry_id}/links/{scene_id}",
    tag = "notebook",
    params(("entry_id" = String, Path), ("scene_id" = String, Path)),
    responses(
        (status = 200, description = "The entry, with its scenes", body = EntryView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn unlink(
    State(state): State<AppState>,
    user: AuthUser,
    Path((entry_id, scene_id)): Path<(String, String)>,
) -> ApiResult<Json<EntryView>> {
    let entry = blocking(&state.pool, move |conn| {
        entry_for(conn, &entry_id, &user, Ability::Write)?;
        diesel::delete(
            scene_notebook_links::table
                .filter(scene_notebook_links::entry_id.eq(&entry_id))
                .filter(scene_notebook_links::scene_id.eq(&scene_id)),
        )
        .execute(conn)?;
        load_entry(conn, &entry_id)
    })
    .await?;
    Ok(Json(entry))
}
