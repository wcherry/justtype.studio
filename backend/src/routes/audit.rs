//! The audit log: publications, permission changes, deletions, and what was
//! done to an account's own security.
//!
//! Separate from a work's activity feed (`collaborators::record`). That one is
//! for the people writing the book and says who edited what; this one is for
//! the record, outlives what it describes, and answers "who let them in" and
//! "who deleted it" after the fact.
use axum::extract::{Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::db::models::{AuditEvent, NewAuditEvent};
use crate::db::schema::{audit_events, work_collaborators, works};
use crate::db::{blocking, Conn};
use crate::domain::enums::AuditAction;
use crate::domain::new_id;
use crate::error::{ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::Page;

pub fn record(
    conn: &mut Conn,
    actor_id: Option<&str>,
    work_id: Option<&str>,
    action: AuditAction,
    detail: &str,
) -> ApiResult<()> {
    diesel::insert_into(audit_events::table)
        .values(&NewAuditEvent {
            id: new_id(),
            actor_id: actor_id.map(str::to_string),
            work_id: work_id.map(str::to_string),
            action: action.as_str().to_string(),
            detail: detail.to_string(),
            created_at: crate::clock::now_iso(),
            subject_id: None,
            reason: None,
        })
        .execute(conn)?;
    Ok(())
}

/// A line in the moderation log: who did it (a user id, or `external:…` for
/// the moderation platform), to which account, and why. One line per action.
pub fn record_staff(
    conn: &mut Conn,
    actor: &str,
    subject_user: Option<&str>,
    action: AuditAction,
    detail: &str,
    reason: Option<&str>,
) -> ApiResult<()> {
    diesel::insert_into(audit_events::table)
        .values(&NewAuditEvent {
            id: new_id(),
            actor_id: Some(actor.to_string()),
            work_id: None,
            action: action.as_str().to_string(),
            detail: detail.to_string(),
            created_at: crate::clock::now_iso(),
            subject_id: subject_user.map(str::to_string),
            reason: reason.map(str::to_string),
        })
        .execute(conn)?;
    Ok(())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

/// What you did, and what anyone did to a work you own. Newest first.
#[utoipa::path(
    get,
    path = "/api/me/audit",
    tag = "trust",
    responses(
        (status = 200, description = "The record", body = Vec<AuditEvent>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<AuditQuery>,
) -> ApiResult<Json<Vec<AuditEvent>>> {
    let (limit, offset) = Page {
        limit: query.limit.unwrap_or(100),
        offset: query.offset.unwrap_or(0),
    }
    .clamped();
    let rows = blocking(&state.pool, move |conn| {
        let owned = works::table
            .filter(works::owner_id.eq(&user.id))
            .select(works::id)
            .load::<String>(conn)?;
        let co_owned = work_collaborators::table
            .filter(work_collaborators::user_id.eq(&user.id))
            .filter(work_collaborators::role.eq("owner"))
            .select(work_collaborators::work_id)
            .load::<String>(conn)?;
        let mine: Vec<String> = owned.into_iter().chain(co_owned).collect();
        Ok(audit_events::table
            .filter(
                audit_events::actor_id
                    .eq(&user.id)
                    .or(audit_events::work_id.eq_any(&mine)),
            )
            .order(audit_events::created_at.desc())
            .limit(limit)
            .offset(offset)
            .select(AuditEvent::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(Json(rows))
}
