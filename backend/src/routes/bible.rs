//! The story bible: characters, places, and the continuity notes that belong to
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
use crate::db::models::{BibleEntry, NewBibleEntry, NewSceneBibleLink};
use crate::db::schema::{bible_entries, scene_bible_links, scenes};
use crate::domain::enums::BibleKind;
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, Ability};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateEntryRequest {
    /// `character`, `place` or `note`.
    #[schema(value_type = crate::domain::enums::BibleKind)]
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEntryRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
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
    pub entry: BibleEntry,
    /// Scenes this entry appears in, in reading order.
    pub scene_ids: Vec<String>,
}

fn load_entry(conn: &mut crate::db::Conn, entry_id: &str) -> ApiResult<EntryView> {
    let entry: BibleEntry = bible_entries::table
        .find(entry_id)
        .select(BibleEntry::as_select())
        .first(conn)?;
    let scene_ids = scene_bible_links::table
        .inner_join(scenes::table.on(scenes::id.eq(scene_bible_links::scene_id)))
        .filter(scene_bible_links::entry_id.eq(entry_id))
        .order((scenes::chapter_id.asc(), scenes::position.asc()))
        .select(scene_bible_links::scene_id)
        .load(conn)?;
    Ok(EntryView { entry, scene_ids })
}

/// The caller owns the work this entry belongs to.
fn entry_for(
    conn: &mut crate::db::Conn,
    entry_id: &str,
    user: &AuthUser,
    need: Ability,
) -> ApiResult<BibleEntry> {
    let entry: BibleEntry = bible_entries::table
        .find(entry_id)
        .select(BibleEntry::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("entry not found".into()))?;
    support::work_for(conn, &entry.work_id, user, need)?;
    Ok(entry)
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/bible",
    tag = "bible",
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
            Some(scene_id) => scene_bible_links::table
                .filter(scene_bible_links::scene_id.eq(scene_id))
                .select(scene_bible_links::entry_id)
                .load(conn)?,
            None => {
                let mut q = bible_entries::table
                    .filter(bible_entries::work_id.eq(&work_id))
                    .into_boxed();
                if let Some(kind) = &query.kind {
                    q = q.filter(bible_entries::kind.eq(kind.clone()));
                }
                q.order((bible_entries::kind.asc(), bible_entries::name.asc()))
                    .select(bible_entries::id)
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
    path = "/api/works/{work_id}/bible",
    tag = "bible",
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
    if !BibleKind::accepts(&body.kind) {
        return Err(ApiError::BadRequest(format!(
            "kind must be one of {}",
            BibleKind::options()
        )));
    }
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::BadRequest("an entry needs a name".into()));
    }

    let entry = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Write)?;
        let now = now_iso();
        let row = NewBibleEntry {
            id: new_id(),
            work_id,
            kind: body.kind.clone(),
            name,
            summary: body.summary.clone(),
            body: body.body.clone(),
            created_at: now.clone(),
            updated_at: now,
        };
        diesel::insert_into(bible_entries::table)
            .values(&row)
            .execute(conn)?;
        load_entry(conn, &row.id)
    })
    .await?;
    Ok(Json(entry))
}

#[utoipa::path(
    patch,
    path = "/api/bible/{entry_id}",
    tag = "bible",
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
            diesel::update(bible_entries::table.find(&entry_id))
                .set(bible_entries::name.eq(name.trim()))
                .execute(conn)?;
        }
        if let Some(summary) = &body.summary {
            diesel::update(bible_entries::table.find(&entry_id))
                .set(bible_entries::summary.eq(summary))
                .execute(conn)?;
        }
        if let Some(text) = &body.body {
            diesel::update(bible_entries::table.find(&entry_id))
                .set(bible_entries::body.eq(text))
                .execute(conn)?;
        }
        diesel::update(bible_entries::table.find(&entry_id))
            .set(bible_entries::updated_at.eq(now_iso()))
            .execute(conn)?;
        load_entry(conn, &entry_id)
    })
    .await?;
    Ok(Json(entry))
}

#[utoipa::path(
    delete,
    path = "/api/bible/{entry_id}",
    tag = "bible",
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
        diesel::delete(bible_entries::table.find(&entry_id)).execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
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
    path = "/api/bible/{entry_id}/links",
    tag = "bible",
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
        diesel::insert_into(scene_bible_links::table)
            .values(&NewSceneBibleLink {
                id: new_id(),
                scene_id: scene.id,
                entry_id: entry_id.clone(),
                created_at: now_iso(),
            })
            .on_conflict((scene_bible_links::scene_id, scene_bible_links::entry_id))
            .do_nothing()
            .execute(conn)?;
        load_entry(conn, &entry_id)
    })
    .await?;
    Ok(Json(entry))
}

#[utoipa::path(
    delete,
    path = "/api/bible/{entry_id}/links/{scene_id}",
    tag = "bible",
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
            scene_bible_links::table
                .filter(scene_bible_links::entry_id.eq(&entry_id))
                .filter(scene_bible_links::scene_id.eq(&scene_id)),
        )
        .execute(conn)?;
        load_entry(conn, &entry_id)
    })
    .await?;
    Ok(Json(entry))
}
