//! Scene statuses: the four every book has, and the writer's own.
//!
//! A book's own statuses come first in its list, in the writer's order, then
//! the built-in ones. Each has a colour, which is what the dot before a scene's
//! title in the outline is drawn in. A scene holds a built-in status by name
//! and one of the book's own by its id, so renaming one never touches a scene.
use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::{CustomStatus, NewCustomStatus};
use crate::db::schema::{scene_statuses, scenes};
use crate::domain::enums::SceneStatus;
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, Ability};

/// The built-in statuses, in the order a scene moves through them, with the
/// colour each is drawn in.
pub const BUILT_IN: &[(SceneStatus, &str, &str)] = &[
    (SceneStatus::Outline, "Outline", "#8e8e93"),
    (SceneStatus::Drafting, "Drafting", "#d4882b"),
    (SceneStatus::Revised, "Revised", "#4a7fc1"),
    (SceneStatus::Done, "Done", "#3d9a5f"),
];

/// Where a scene goes when the status it had is removed.
pub const FALLBACK: &str = "drafting";

const MAX_CUSTOM: usize = 30;
const MAX_LABEL_CHARS: usize = 40;

/// The colours offered for a book's own statuses, in the order a new one is
/// given them. Any `#rrggbb` is accepted; these are the ones that read on both
/// the light and the dark theme.
pub const PALETTE: &[&str] = &[
    "#c2453d", "#b5649a", "#7a5cc2", "#2f9aa8", "#6b8e23", "#a0522d", "#c9a227", "#5b6b7a",
];

/// A status a scene can have.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusOption {
    /// What a scene's `status` holds: the name of a built-in status, or the
    /// id of one of the book's own.
    pub key: String,
    pub label: String,
    /// `#rrggbb`.
    pub color: String,
    /// One of the book's own, which can be renamed, recoloured, reordered and
    /// removed; the built-in ones cannot.
    pub custom: bool,
}

/// Every status a scene in this work can have: the book's own first, then the
/// built-in ones.
pub fn for_work(conn: &mut crate::db::Conn, work_id: &str) -> ApiResult<Vec<StatusOption>> {
    let mut out: Vec<StatusOption> = custom_rows(conn, work_id)?
        .into_iter()
        .map(|row| StatusOption {
            key: row.id,
            label: row.label,
            color: row.color,
            custom: true,
        })
        .collect();
    out.extend(BUILT_IN.iter().map(|(status, label, color)| StatusOption {
        key: status.as_str().into(),
        label: (*label).into(),
        color: (*color).into(),
        custom: false,
    }));
    Ok(out)
}

/// Whether a scene in this work may have this status.
pub fn accepts(conn: &mut crate::db::Conn, work_id: &str, status: &str) -> ApiResult<bool> {
    if SceneStatus::accepts(status) {
        return Ok(true);
    }
    Ok(scene_statuses::table
        .filter(scene_statuses::id.eq(status))
        .filter(scene_statuses::work_id.eq(work_id))
        .count()
        .get_result::<i64>(conn)?
        > 0)
}

pub fn custom_rows(conn: &mut crate::db::Conn, work_id: &str) -> ApiResult<Vec<CustomStatus>> {
    Ok(scene_statuses::table
        .filter(scene_statuses::work_id.eq(work_id))
        .order((
            scene_statuses::position.asc(),
            scene_statuses::created_at.asc(),
        ))
        .select(CustomStatus::as_select())
        .load(conn)?)
}

/// Adds one of the writer's own statuses to a book, at the end of them. Used
/// by the handler and by an import, which has already checked the work.
pub fn insert(
    conn: &mut crate::db::Conn,
    work_id: &str,
    label: &str,
    color: &str,
) -> ApiResult<String> {
    let last: Option<i32> = scene_statuses::table
        .filter(scene_statuses::work_id.eq(work_id))
        .select(diesel::dsl::max(scene_statuses::position))
        .first(conn)?;
    let id = new_id();
    diesel::insert_into(scene_statuses::table)
        .values(&NewCustomStatus {
            id: id.clone(),
            work_id: work_id.into(),
            label: label.into(),
            color: color.to_ascii_lowercase(),
            position: last.map_or(0, |p| p + 1),
            created_at: now_iso(),
        })
        .execute(conn)?;
    Ok(id)
}

pub fn valid_color(color: &str) -> bool {
    color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|c| c.is_ascii_hexdigit())
}

fn checked_color(color: &str) -> ApiResult<String> {
    let color = color.trim();
    if !valid_color(color) {
        return Err(ApiError::BadRequest(
            "a colour is `#` and six hex digits, like #c2453d".into(),
        ));
    }
    Ok(color.to_ascii_lowercase())
}

/// A name that is not empty, not too long, and not one the book already has —
/// two statuses called "Needs research" could not be told apart in a menu.
fn checked_label(
    conn: &mut crate::db::Conn,
    work_id: &str,
    label: &str,
    except: Option<&str>,
) -> ApiResult<String> {
    let label = label.trim();
    if label.is_empty() {
        return Err(ApiError::BadRequest("a status needs a name".into()));
    }
    if label.chars().count() > MAX_LABEL_CHARS {
        return Err(ApiError::BadRequest(format!(
            "a status's name is at most {MAX_LABEL_CHARS} characters"
        )));
    }
    let taken = for_work(conn, work_id)?
        .into_iter()
        .filter(|s| Some(s.key.as_str()) != except)
        .any(|s| s.label.to_lowercase() == label.to_lowercase());
    if taken {
        return Err(ApiError::Conflict(format!(
            "this book already has a status called “{label}”"
        )));
    }
    Ok(label.to_string())
}

/// The book's own status, with the caller allowed to change the book.
fn status_for(
    conn: &mut crate::db::Conn,
    status_id: &str,
    user: &AuthUser,
) -> ApiResult<CustomStatus> {
    let row: CustomStatus = scene_statuses::table
        .find(status_id)
        .select(CustomStatus::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("status not found".into()))?;
    support::work_for(conn, &row.work_id, user, Ability::Write)?;
    Ok(row)
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/statuses",
    tag = "statuses",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "The book's statuses: its own, then the built-in ones", body = Vec<StatusOption>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Vec<StatusOption>>> {
    let list = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Read)?;
        for_work(conn, &work_id)
    })
    .await?;
    Ok(Json(list))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateStatusRequest {
    pub label: String,
    /// `#rrggbb`. Left out, it gets the next colour in the palette.
    #[serde(default)]
    pub color: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/statuses",
    tag = "statuses",
    params(("work_id" = String, Path)),
    request_body = CreateStatusRequest,
    responses(
        (status = 200, description = "The book's statuses, with the new one after its others", body = Vec<StatusOption>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<CreateStatusRequest>,
) -> ApiResult<Json<Vec<StatusOption>>> {
    let color = body.color.as_deref().map(checked_color).transpose()?;
    let list = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Write)?;
        let existing = custom_rows(conn, &work_id)?;
        if existing.len() >= MAX_CUSTOM {
            return Err(ApiError::BadRequest(format!(
                "a book has at most {MAX_CUSTOM} statuses of its own"
            )));
        }
        let label = checked_label(conn, &work_id, &body.label, None)?;
        let color = color.unwrap_or_else(|| PALETTE[existing.len() % PALETTE.len()].into());
        insert(conn, &work_id, &label, &color)?;
        for_work(conn, &work_id)
    })
    .await?;
    Ok(Json(list))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusRequest {
    #[serde(default)]
    pub label: Option<String>,
    /// `#rrggbb`.
    #[serde(default)]
    pub color: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/api/statuses/{status_id}",
    tag = "statuses",
    params(("status_id" = String, Path)),
    request_body = UpdateStatusRequest,
    responses(
        (status = 200, description = "The book's statuses", body = Vec<StatusOption>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(status_id): Path<String>,
    Json(body): Json<UpdateStatusRequest>,
) -> ApiResult<Json<Vec<StatusOption>>> {
    let color = body.color.as_deref().map(checked_color).transpose()?;
    let list = blocking(&state.pool, move |conn| {
        let row = status_for(conn, &status_id, &user)?;
        if let Some(label) = &body.label {
            let label = checked_label(conn, &row.work_id, label, Some(&row.id))?;
            diesel::update(scene_statuses::table.find(&row.id))
                .set(scene_statuses::label.eq(label))
                .execute(conn)?;
        }
        if let Some(color) = color {
            diesel::update(scene_statuses::table.find(&row.id))
                .set(scene_statuses::color.eq(color))
                .execute(conn)?;
        }
        for_work(conn, &row.work_id)
    })
    .await?;
    Ok(Json(list))
}

/// Removes one of the book's own statuses. Its scenes go back to drafting.
#[utoipa::path(
    delete,
    path = "/api/statuses/{status_id}",
    tag = "statuses",
    params(("status_id" = String, Path)),
    responses(
        (status = 200, description = "The book's statuses, without it", body = Vec<StatusOption>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_status(
    State(state): State<AppState>,
    user: AuthUser,
    Path(status_id): Path<String>,
) -> ApiResult<Json<Vec<StatusOption>>> {
    let list = blocking(&state.pool, move |conn| {
        let row = status_for(conn, &status_id, &user)?;
        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::update(
                scenes::table
                    .filter(scenes::work_id.eq(&row.work_id))
                    .filter(scenes::status.eq(&row.id)),
            )
            .set(scenes::status.eq(FALLBACK))
            .execute(conn)?;
            diesel::delete(scene_statuses::table.find(&row.id)).execute(conn)?;
            Ok(())
        })?;
        for_work(conn, &row.work_id)
    })
    .await?;
    Ok(Json(list))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReorderStatusesRequest {
    /// The book's own statuses in their new order. Any left out keep their
    /// order among themselves, after the ones listed.
    pub ids: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/statuses/reorder",
    tag = "statuses",
    params(("work_id" = String, Path)),
    request_body = ReorderStatusesRequest,
    responses(
        (status = 200, description = "The book's statuses, in their new order", body = Vec<StatusOption>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn reorder(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<ReorderStatusesRequest>,
) -> ApiResult<Json<Vec<StatusOption>>> {
    let list = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Write)?;
        let current: Vec<String> = custom_rows(conn, &work_id)?
            .into_iter()
            .map(|r| r.id)
            .collect();
        if let Some(stray) = body.ids.iter().find(|id| !current.contains(id)) {
            return Err(ApiError::BadRequest(format!(
                "`{stray}` is not one of this book's own statuses"
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
                diesel::update(scene_statuses::table.find(*id))
                    .set(scene_statuses::position.eq(index as i32))
                    .execute(conn)?;
            }
            Ok(())
        })?;
        for_work(conn, &work_id)
    })
    .await?;
    Ok(Json(list))
}
