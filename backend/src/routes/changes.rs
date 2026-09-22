//! Tracked changes: proposed edits that sit beside a scene until the author
//! accepts or rejects them.
//!
//! The server stores the proposal and its resolution; applying an accepted
//! change to the document is the editor's job, because only ProseMirror knows
//! how to map a position through the edits made since the proposal was written.
//! The client accepts, applies, and PUTs the new scene content.
use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::Deserialize;
use serde_json::Value;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::{NewTrackedChange, TrackedChange};
use crate::db::schema::tracked_changes;
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

use super::support;

const CHANGE_KINDS: &[&str] = &["insert", "delete", "replace", "format"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    /// `pending` (default), `accepted`, `rejected`, or `all`.
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChangeRequest {
    pub kind: String,
    pub anchor_from: i32,
    pub anchor_to: i32,
    #[serde(default)]
    pub quote: Option<String>,
    /// Editor-defined slice or mark spec; opaque to the server.
    #[serde(default)]
    pub payload: Option<Value>,
    #[serde(default)]
    pub note: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Vec<TrackedChange>>> {
    let wanted = query.state.unwrap_or_else(|| "pending".into());
    let rows = blocking(&state.pool, move |conn| {
        let (scene, _) = support::scene_for_owner(conn, &scene_id, &user)?;
        let mut q = tracked_changes::table
            .filter(tracked_changes::scene_id.eq(&scene.id))
            .into_boxed();
        if wanted != "all" {
            q = q.filter(tracked_changes::state.eq(wanted));
        }
        Ok(q.order(tracked_changes::created_at.asc())
            .select(TrackedChange::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(Json(rows))
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Json(body): Json<CreateChangeRequest>,
) -> ApiResult<Json<TrackedChange>> {
    if !CHANGE_KINDS.contains(&body.kind.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "kind must be one of {}",
            CHANGE_KINDS.join(", ")
        )));
    }
    if body.anchor_to < body.anchor_from {
        return Err(ApiError::BadRequest(
            "anchorTo must not precede anchorFrom".into(),
        ));
    }

    let change = blocking(&state.pool, move |conn| {
        let (scene, work) = support::scene_for_owner(conn, &scene_id, &user)?;
        let row = NewTrackedChange {
            id: new_id(),
            scene_id: scene.id.clone(),
            work_id: work.id,
            author_id: user.id.clone(),
            kind: body.kind.clone(),
            anchor_from: body.anchor_from,
            anchor_to: body.anchor_to,
            quote: body.quote.clone(),
            payload: body.payload.clone().unwrap_or(Value::Null).to_string(),
            note: body.note.clone(),
            state: "pending".into(),
            resolved_by: None,
            resolved_at: None,
            created_at: now_iso(),
        };
        diesel::insert_into(tracked_changes::table)
            .values(&row)
            .execute(conn)?;
        Ok(tracked_changes::table
            .find(&row.id)
            .select(TrackedChange::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(change))
}

pub async fn accept(
    State(state): State<AppState>,
    user: AuthUser,
    Path(change_id): Path<String>,
) -> ApiResult<Json<TrackedChange>> {
    resolve(state, user, change_id, "accepted").await
}

pub async fn reject(
    State(state): State<AppState>,
    user: AuthUser,
    Path(change_id): Path<String>,
) -> ApiResult<Json<TrackedChange>> {
    resolve(state, user, change_id, "rejected").await
}

async fn resolve(
    state: AppState,
    user: AuthUser,
    change_id: String,
    new_state: &'static str,
) -> ApiResult<Json<TrackedChange>> {
    let change = blocking(&state.pool, move |conn| {
        let change: TrackedChange = tracked_changes::table
            .find(&change_id)
            .select(TrackedChange::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("change not found".into()))?;
        // Only the author of the work decides what lands in it.
        support::work_for_owner(conn, &change.work_id, &user)?;
        if change.state != "pending" {
            return Err(ApiError::Conflict(format!(
                "change was already {}",
                change.state
            )));
        }
        diesel::update(tracked_changes::table.find(&change.id))
            .set((
                tracked_changes::state.eq(new_state),
                tracked_changes::resolved_by.eq(&user.id),
                tracked_changes::resolved_at.eq(now_iso()),
            ))
            .execute(conn)?;
        Ok(tracked_changes::table
            .find(&change.id)
            .select(TrackedChange::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(change))
}
