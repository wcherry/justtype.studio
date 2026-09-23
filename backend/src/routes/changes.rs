//! Tracked changes: proposed edits that sit beside a scene until the author
//! accepts or rejects them.
//!
//! Accepting one applies it, here. That used to be the editor's job, on the
//! reasoning that only ProseMirror could map a position through later edits —
//! but it meant "accepted" behaved differently on every client, could not work
//! from a phone, and could not accept twenty at once. The mapping turns out to
//! be tractable without a full transform history:
//!
//! * a proposal records the `contentHash` it was written against, so an
//!   unchanged scene applies its positions exactly;
//! * a changed scene re-anchors by the quoted text, which is unambiguous when
//!   the quotation still appears exactly once;
//! * anything else is refused as stale rather than guessed at.
//!
//! Batches apply from the end of the document backwards, so accepting one never
//! invalidates the positions of the ones not yet applied.
use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::{NewTrackedChange, Scene, TrackedChange};
use crate::db::schema::{scenes, tracked_changes};
use crate::domain::enums::ChangeKind;
use crate::domain::{doc, edit, new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    /// `pending` (default), `accepted`, `rejected`, or `all`.
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateChangeRequest {
    pub kind: String,
    pub anchor_from: i32,
    pub anchor_to: i32,
    #[serde(default)]
    pub quote: Option<String>,
    /// The replacement text for `insert` and `replace`, or the mark name for
    /// `format`. Ignored for `delete`.
    #[serde(default)]
    pub payload: Option<Value>,
    #[serde(default)]
    pub note: Option<String>,
}

/// Why a change could not be applied, for a client that has to explain it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StaleChange {
    pub change_id: String,
    pub reason: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveResponse {
    pub change: TrackedChange,
    /// The scene as it stands after applying it, when one was applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scene: Option<Scene>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkResolveResponse {
    pub applied: usize,
    pub rejected: usize,
    /// Proposals whose text has moved on too far to place automatically. They
    /// stay pending: refusing is safer than guessing where they meant.
    pub stale: Vec<StaleChange>,
    pub scene: Scene,
}

/// Where a proposal's positions land in the document as it stands now.
fn locate(
    change: &TrackedChange,
    document: &Value,
    current_hash: &str,
) -> Result<(usize, usize), String> {
    let from = change.anchor_from.max(0) as usize;
    let to = change.anchor_to.max(0) as usize;

    // Unchanged since the proposal: the positions are exactly right.
    if change.base_hash.as_deref() == Some(current_hash) {
        return Ok((from, to));
    }

    // Moved on. The quotation is the only thing that still means anything.
    let Some(quote) = change.quote.as_deref().filter(|q| !q.is_empty()) else {
        return Err(
            "the scene changed since this was proposed, and it has no quoted text to find again"
                .into(),
        );
    };
    match edit::find_quote(document, quote).as_slice() {
        [single] => Ok(*single),
        [] => Err(format!("`{quote}` is no longer in this scene")),
        many => Err(format!(
            "`{quote}` now appears {} times, so where this change belongs is ambiguous",
            many.len()
        )),
    }
}

/// Applies one proposal to a document, returning the new document.
fn apply(change: &TrackedChange, document: &Value, current_hash: &str) -> Result<Value, String> {
    let (from, to) = locate(change, document, current_hash)?;
    let payload_text = change
        .payload
        .parse::<Value>()
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .or_else(|| {
            // A payload that is not JSON is taken as the literal text, which is
            // what a simple client is most likely to send.
            (!change.payload.is_empty() && change.payload != "null").then(|| change.payload.clone())
        })
        .unwrap_or_default();

    let result = match change.kind.as_str() {
        "insert" => edit::replace_range(document, from, from, &payload_text),
        "delete" => edit::replace_range(document, from, to, ""),
        "replace" => edit::replace_range(document, from, to, &payload_text),
        "format" => {
            let mark = if payload_text.is_empty() {
                "italic"
            } else {
                payload_text.as_str()
            };
            edit::set_mark(document, from, to, mark, true)
        }
        other => return Err(format!("unknown change kind `{other}`")),
    };
    result.map_err(|e| e.to_string())
}

#[utoipa::path(
    get,
    path = "/api/scenes/{scene_id}/changes",
    tag = "changes",
    params(("scene_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = Vec<TrackedChange>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
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

#[utoipa::path(
    post,
    path = "/api/scenes/{scene_id}/changes",
    tag = "changes",
    params(("scene_id" = String, Path)),
    request_body = CreateChangeRequest,
    responses(
        (status = 200, description = "Success", body = TrackedChange),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Json(body): Json<CreateChangeRequest>,
) -> ApiResult<Json<TrackedChange>> {
    if !ChangeKind::accepts(&body.kind) {
        return Err(ApiError::BadRequest(format!(
            "kind must be one of {}",
            ChangeKind::options()
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
            base_hash: Some(scene.content_hash.clone()),
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

#[utoipa::path(
    post,
    path = "/api/changes/{change_id}/accept",
    tag = "changes",
    params(("change_id" = String, Path)),
    responses(
        (status = 200, description = "The change, and the scene it was applied to", body = ResolveResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn accept(
    State(state): State<AppState>,
    user: AuthUser,
    Path(change_id): Path<String>,
) -> ApiResult<Json<ResolveResponse>> {
    resolve_one(state, user, change_id, true).await
}

#[utoipa::path(
    post,
    path = "/api/changes/{change_id}/reject",
    tag = "changes",
    params(("change_id" = String, Path)),
    responses(
        (status = 200, description = "The change", body = ResolveResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn reject(
    State(state): State<AppState>,
    user: AuthUser,
    Path(change_id): Path<String>,
) -> ApiResult<Json<ResolveResponse>> {
    resolve_one(state, user, change_id, false).await
}

/// Loads a pending change and the scene it belongs to, checking ownership.
fn pending_change(
    conn: &mut crate::db::Conn,
    change_id: &str,
    user: &AuthUser,
) -> ApiResult<(TrackedChange, Scene)> {
    let change: TrackedChange = tracked_changes::table
        .find(change_id)
        .select(TrackedChange::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("change not found".into()))?;
    // Only the author of the work decides what lands in it.
    support::work_for_owner(conn, &change.work_id, user)?;
    if change.state != "pending" {
        return Err(ApiError::Conflict(format!(
            "change was already {}",
            change.state
        )));
    }
    let (scene, _) = support::scene_for_owner(conn, &change.scene_id, user)?;
    Ok((change, scene))
}

/// Writes a scene the server edited, and keeps the counts honest.
async fn store_scene(state: &AppState, scene: &Scene, document: &Value) -> ApiResult<Scene> {
    state
        .store
        .write_working_scene(&scene.work_id, &scene.id, document)
        .await?;

    let hash = doc::content_hash(document);
    let words = doc::word_count(document);
    let scene_id = scene.id.clone();
    let work_id = scene.work_id.clone();
    blocking(&state.pool, move |conn| {
        let now = now_iso();
        diesel::update(scenes::table.find(&scene_id))
            .set((
                scenes::content_hash.eq(&hash),
                scenes::word_count.eq(words),
                scenes::updated_at.eq(&now),
            ))
            .execute(conn)?;
        support::recount_work(conn, &work_id, &now)?;
        Ok(scenes::table
            .find(&scene_id)
            .select(Scene::as_select())
            .first(conn)?)
    })
    .await
}

fn mark_resolved(
    conn: &mut crate::db::Conn,
    change_id: &str,
    user: &AuthUser,
    accepted: bool,
) -> ApiResult<TrackedChange> {
    diesel::update(tracked_changes::table.find(change_id))
        .set((
            tracked_changes::state.eq(if accepted { "accepted" } else { "rejected" }),
            tracked_changes::resolved_by.eq(&user.id),
            tracked_changes::resolved_at.eq(now_iso()),
        ))
        .execute(conn)?;
    Ok(tracked_changes::table
        .find(change_id)
        .select(TrackedChange::as_select())
        .first(conn)?)
}

async fn resolve_one(
    state: AppState,
    user: AuthUser,
    change_id: String,
    accepted: bool,
) -> ApiResult<Json<ResolveResponse>> {
    let lookup_user = user.clone();
    let lookup_id = change_id.clone();
    let (change, scene) = blocking(&state.pool, move |conn| {
        pending_change(conn, &lookup_id, &lookup_user)
    })
    .await?;

    // Rejecting touches no text, so nothing can go wrong with it.
    if !accepted {
        let resolve_user = user.clone();
        let resolve_id = change_id.clone();
        let change = blocking(&state.pool, move |conn| {
            mark_resolved(conn, &resolve_id, &resolve_user, false)
        })
        .await?;
        return Ok(Json(ResolveResponse {
            change,
            scene: None,
        }));
    }

    let document = state
        .store
        .read_working_scene(&scene.work_id, &scene.id)
        .await?;
    let edited = apply(&change, &document, &scene.content_hash).map_err(ApiError::Conflict)?;
    let scene = store_scene(&state, &scene, &edited).await?;

    let change = blocking(&state.pool, move |conn| {
        mark_resolved(conn, &change_id, &user, true)
    })
    .await?;

    Ok(Json(ResolveResponse {
        change,
        scene: Some(scene),
    }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkResolveRequest {
    /// `accept` or `reject`.
    pub action: String,
    /// Only this author's proposals, when set.
    #[serde(default)]
    pub author_id: Option<String>,
}

/// Resolves every pending change in a scene at once.
///
/// Accepts apply from the end of the document backwards, so applying one never
/// shifts the positions of the ones still to come. A proposal that cannot be
/// placed is left pending and reported, rather than skipped silently or applied
/// somewhere plausible.
#[utoipa::path(
    post,
    path = "/api/scenes/{scene_id}/changes/resolve",
    tag = "changes",
    params(("scene_id" = String, Path)),
    request_body = BulkResolveRequest,
    responses(
        (status = 200, description = "What happened to each", body = BulkResolveResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn resolve_all(
    State(state): State<AppState>,
    user: AuthUser,
    Path(scene_id): Path<String>,
    Json(body): Json<BulkResolveRequest>,
) -> ApiResult<Json<BulkResolveResponse>> {
    let accepted = match body.action.as_str() {
        "accept" => true,
        "reject" => false,
        other => {
            return Err(ApiError::BadRequest(format!(
                "action must be `accept` or `reject`, got `{other}`"
            )))
        }
    };

    let lookup_user = user.clone();
    let lookup_id = scene_id.clone();
    let author = body.author_id.clone();
    let (scene, mut pending) = blocking(&state.pool, move |conn| {
        let (scene, _) = support::scene_for_owner(conn, &lookup_id, &lookup_user)?;
        let mut query = tracked_changes::table
            .filter(tracked_changes::scene_id.eq(&scene.id))
            .filter(tracked_changes::state.eq("pending"))
            .into_boxed();
        if let Some(author) = &author {
            query = query.filter(tracked_changes::author_id.eq(author.clone()));
        }
        let pending: Vec<TrackedChange> = query
            .order(tracked_changes::anchor_from.desc())
            .select(TrackedChange::as_select())
            .load(conn)?;
        Ok((scene, pending))
    })
    .await?;

    // Back to front: an edit late in the document cannot disturb an earlier one.
    pending.sort_by_key(|change| std::cmp::Reverse(change.anchor_from));

    let mut document = state
        .store
        .read_working_scene(&scene.work_id, &scene.id)
        .await?;
    let mut current_hash = scene.content_hash.clone();
    let mut applied = Vec::new();
    let mut stale = Vec::new();

    for change in &pending {
        if !accepted {
            applied.push(change.id.clone());
            continue;
        }
        match apply(change, &document, &current_hash) {
            Ok(edited) => {
                document = edited;
                // Each application moves the document on, so the next change's
                // base no longer matches and it re-anchors by quotation — which
                // is the right behaviour, not a fallback.
                current_hash = doc::content_hash(&document);
                applied.push(change.id.clone());
            }
            Err(reason) => stale.push(StaleChange {
                change_id: change.id.clone(),
                reason,
            }),
        }
    }

    let scene = if accepted && !applied.is_empty() {
        store_scene(&state, &scene, &document).await?
    } else {
        scene
    };

    let resolved_count = applied.len();
    let resolve_user = user.clone();
    blocking(&state.pool, move |conn| {
        let now = now_iso();
        for id in &applied {
            diesel::update(tracked_changes::table.find(id))
                .set((
                    tracked_changes::state.eq(if accepted { "accepted" } else { "rejected" }),
                    tracked_changes::resolved_by.eq(&resolve_user.id),
                    tracked_changes::resolved_at.eq(&now),
                ))
                .execute(conn)?;
        }
        Ok(())
    })
    .await?;

    Ok(Json(BulkResolveResponse {
        applied: if accepted { resolved_count } else { 0 },
        rejected: if accepted { 0 } else { resolved_count },
        stale,
        scene,
    }))
}
