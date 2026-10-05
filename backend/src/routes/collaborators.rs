//! Co-authors on a work, and what they have been doing to it.
//!
//! A collaborator is addressed by email, like an invited reader: the person a
//! writer wants on the book may not have an account yet. What they may do is
//! the role — owner, writer, editor or reader — optionally narrowed to some
//! chapters for a ghostwriter or an editorial hand-off. The rules themselves
//! live in `support::Access`; this file only keeps the list.
use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::models::*;
use crate::db::schema::{
    chapters, collaborator_chapters, users, work_activity, work_collaborators,
};
use crate::db::{blocking, Conn};
use crate::domain::enums::{ActivityKind, CollaboratorRole};
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, Ability, AuthorCard, Page};

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollaboratorView {
    pub id: String,
    #[schema(value_type = CollaboratorRole)]
    pub role: String,
    /// The chapters they may change, when they were handed only some. Absent
    /// means the whole work.
    pub chapter_ids: Option<Vec<String>>,
    /// Who they are, once they have an account and have opened the work.
    pub person: Option<AuthorCard>,
    /// The address it went to. Shown to owners only: the people on a book do
    /// not need each other's addresses to work on it.
    pub email: Option<String>,
    pub accepted_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddCollaboratorRequest {
    pub email: String,
    #[schema(value_type = CollaboratorRole)]
    pub role: String,
    /// Narrow what they may change to these chapters. Omitted, the whole work.
    #[serde(default)]
    pub chapter_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCollaboratorRequest {
    #[serde(default)]
    #[schema(value_type = Option<CollaboratorRole>)]
    pub role: Option<String>,
    /// Narrow them to exactly these chapters. An empty list leaves them
    /// nowhere to write, which is not the same as everywhere.
    #[serde(default)]
    pub chapter_ids: Option<Vec<String>>,
    /// `true` hands them the whole work again.
    #[serde(default)]
    pub whole_work: Option<bool>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityView {
    #[serde(flatten)]
    pub activity: WorkActivity,
    pub actor: AuthorCard,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActivityQuery {
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

// --- the list ------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/collaborators",
    tag = "collaboration",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "Who else is on this work", body = Vec<CollaboratorView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Vec<CollaboratorView>>> {
    let views = blocking(&state.pool, move |conn| {
        let (work, access) = support::work_access(conn, &work_id, &user)?;
        views(conn, &work.id, access.can(Ability::Own))
    })
    .await?;
    Ok(Json(views))
}

fn views(conn: &mut Conn, work_id: &str, owner: bool) -> ApiResult<Vec<CollaboratorView>> {
    let rows: Vec<WorkCollaborator> = work_collaborators::table
        .filter(work_collaborators::work_id.eq(work_id))
        .order(work_collaborators::created_at.asc())
        .select(WorkCollaborator::as_select())
        .load(conn)?;
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let mut granted: HashMap<String, Vec<String>> = HashMap::new();
    for (collaborator_id, chapter_id) in collaborator_chapters::table
        .filter(collaborator_chapters::collaborator_id.eq_any(&ids))
        .select((
            collaborator_chapters::collaborator_id,
            collaborator_chapters::chapter_id,
        ))
        .load::<(String, String)>(conn)?
    {
        granted.entry(collaborator_id).or_default().push(chapter_id);
    }
    let people: Vec<String> = rows.iter().filter_map(|r| r.user_id.clone()).collect();
    let cards = support::author_cards(conn, &people)?;

    Ok(rows
        .into_iter()
        .map(|row| CollaboratorView {
            chapter_ids: (row.chapter_scoped != 0).then(|| {
                let mut ids = granted.remove(&row.id).unwrap_or_default();
                ids.sort();
                ids
            }),
            person: row.user_id.as_ref().and_then(|id| cards.get(id).cloned()),
            email: owner.then_some(row.email),
            id: row.id,
            role: row.role,
            accepted_at: row.accepted_at,
            created_at: row.created_at,
        })
        .collect())
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/collaborators",
    tag = "collaboration",
    params(("work_id" = String, Path)),
    request_body = AddCollaboratorRequest,
    responses(
        (status = 200, description = "Everyone on the work, the new one included", body = Vec<CollaboratorView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn add(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<AddCollaboratorRequest>,
) -> ApiResult<Json<Vec<CollaboratorView>>> {
    check_role(&body.role)?;
    let email = body.email.trim().to_lowercase();
    if !email.contains('@') || email.len() < 5 {
        return Err(ApiError::BadRequest(
            "a valid email address is required".into(),
        ));
    }

    let views = blocking(&state.pool, move |conn| {
        let work = support::work_for_owner(conn, &work_id, &user)?;
        let creator_email: String = users::table
            .find(&work.owner_id)
            .select(users::email)
            .first(conn)?;
        if email == creator_email {
            return Err(ApiError::BadRequest(
                "that is the person who started this work — they already have all of it".into(),
            ));
        }
        let chapters = checked_chapters(conn, &work.id, &body.role, body.chapter_ids.as_deref())?;

        let existing: Option<String> = work_collaborators::table
            .filter(work_collaborators::work_id.eq(&work.id))
            .filter(work_collaborators::email.eq(&email))
            .select(work_collaborators::id)
            .first(conn)
            .optional()?;
        if existing.is_some() {
            return Err(ApiError::Conflict(
                "they are already on this work — change their role instead".into(),
            ));
        }

        let now = now_iso();
        // If they already have an account, say so now rather than on their
        // first visit, so the list can show a name rather than an address.
        let known: Option<String> = users::table
            .filter(users::email.eq(&email))
            .select(users::id)
            .first(conn)
            .optional()?;
        let row = NewWorkCollaborator {
            id: new_id(),
            work_id: work.id.clone(),
            email: email.clone(),
            user_id: known,
            role: body.role.clone(),
            chapter_scoped: i32::from(chapters.is_some()),
            invited_by: Some(user.id.clone()),
            accepted_at: None,
            created_at: now.clone(),
            updated_at: now,
        };
        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::insert_into(work_collaborators::table)
                .values(&row)
                .execute(conn)?;
            grant(conn, &row.id, chapters.as_deref().unwrap_or_default())?;
            let name = who(conn, row.user_id.as_deref())?;
            record(
                conn,
                &work.id,
                &user.id,
                ActivityKind::CollaboratorAdded,
                Some(&row.id),
                &format!("{} as {}", name, body.role),
            )
        })?;
        views(conn, &work.id, true)
    })
    .await?;
    Ok(Json(views))
}

#[utoipa::path(
    patch,
    path = "/api/works/{work_id}/collaborators/{collaborator_id}",
    tag = "collaboration",
    params(("work_id" = String, Path), ("collaborator_id" = String, Path)),
    request_body = UpdateCollaboratorRequest,
    responses(
        (status = 200, description = "Everyone on the work", body = Vec<CollaboratorView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path((work_id, collaborator_id)): Path<(String, String)>,
    Json(body): Json<UpdateCollaboratorRequest>,
) -> ApiResult<Json<Vec<CollaboratorView>>> {
    if let Some(role) = &body.role {
        check_role(role)?;
    }
    if body.whole_work == Some(true) && body.chapter_ids.is_some() {
        return Err(ApiError::BadRequest(
            "send chapterIds or wholeWork, not both".into(),
        ));
    }

    let views = blocking(&state.pool, move |conn| {
        let work = support::work_for_owner(conn, &work_id, &user)?;
        let row = find(conn, &work.id, &collaborator_id)?;
        let role = body.role.clone().unwrap_or_else(|| row.role.clone());
        // Where they may write afterwards: what was asked for, or what they
        // had. An owner always has the whole work.
        let wanted: Option<Vec<String>> = match (&body.chapter_ids, body.whole_work) {
            (Some(ids), _) => Some(ids.clone()),
            (None, Some(true)) => None,
            _ if role == "owner" => None,
            _ if row.chapter_scoped != 0 => Some(
                collaborator_chapters::table
                    .filter(collaborator_chapters::collaborator_id.eq(&row.id))
                    .select(collaborator_chapters::chapter_id)
                    .load(conn)?,
            ),
            _ => None,
        };
        let chapters = checked_chapters(conn, &work.id, &role, wanted.as_deref())?;

        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::update(work_collaborators::table.find(&row.id))
                .set((
                    work_collaborators::role.eq(&role),
                    work_collaborators::chapter_scoped.eq(i32::from(chapters.is_some())),
                    work_collaborators::updated_at.eq(now_iso()),
                ))
                .execute(conn)?;
            diesel::delete(
                collaborator_chapters::table
                    .filter(collaborator_chapters::collaborator_id.eq(&row.id)),
            )
            .execute(conn)?;
            grant(conn, &row.id, chapters.as_deref().unwrap_or_default())?;
            let reach = match &chapters {
                None => String::new(),
                Some(ids) => format!(
                    ", {} chapter{}",
                    ids.len(),
                    if ids.len() == 1 { "" } else { "s" }
                ),
            };
            let name = who(conn, row.user_id.as_deref())?;
            record(
                conn,
                &work.id,
                &user.id,
                ActivityKind::CollaboratorChanged,
                Some(&row.id),
                &format!("{} now {role}{reach}", name),
            )
        })?;
        views(conn, &work.id, true)
    })
    .await?;
    Ok(Json(views))
}

#[utoipa::path(
    delete,
    path = "/api/works/{work_id}/collaborators/{collaborator_id}",
    tag = "collaboration",
    params(("work_id" = String, Path), ("collaborator_id" = String, Path)),
    responses(
        (status = 200, description = "Removed, or left", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path((work_id, collaborator_id)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    blocking(&state.pool, move |conn| {
        let (work, access) = support::work_access(conn, &work_id, &user)?;
        let row = find(conn, &work.id, &collaborator_id)?;
        // Anyone may step off a book; only an owner may take someone else off.
        let leaving = row.email == user.email.trim().to_lowercase();
        if !leaving && !access.can(Ability::Own) {
            return Err(ApiError::Forbidden(
                "only an owner of this work can do that".into(),
            ));
        }
        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::delete(work_collaborators::table.find(&row.id)).execute(conn)?;
            let name = who(conn, row.user_id.as_deref())?;
            record(
                conn,
                &work.id,
                &user.id,
                ActivityKind::CollaboratorRemoved,
                Some(&row.id),
                &name,
            )
        })
    })
    .await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

fn find(conn: &mut Conn, work_id: &str, collaborator_id: &str) -> ApiResult<WorkCollaborator> {
    work_collaborators::table
        .find(collaborator_id)
        .filter(work_collaborators::work_id.eq(work_id))
        .select(WorkCollaborator::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("no such collaborator on this work".into()))
}

/// How a collaborator is named in the activity feed. Never by address: every
/// collaborator reads the feed, and only owners are shown addresses.
fn who(conn: &mut Conn, user_id: Option<&str>) -> ApiResult<String> {
    let name = match user_id {
        Some(id) => users::table
            .find(id)
            .select(users::display_name)
            .first::<String>(conn)
            .optional()?,
        None => None,
    };
    Ok(name.unwrap_or_else(|| "someone not yet signed up".into()))
}

fn check_role(role: &str) -> ApiResult<()> {
    if CollaboratorRole::accepts(role) {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "role must be one of {}",
            CollaboratorRole::options()
        )))
    }
}

/// The chapters asked for, checked against the work. `None` is the whole work.
fn checked_chapters(
    conn: &mut Conn,
    work_id: &str,
    role: &str,
    wanted: Option<&[String]>,
) -> ApiResult<Option<Vec<String>>> {
    let Some(wanted) = wanted else {
        return Ok(None);
    };
    if role == "owner" {
        return Err(ApiError::BadRequest(
            "an owner has the whole work — narrow a writer or an editor instead".into(),
        ));
    }
    let mut ids: Vec<String> = wanted.to_vec();
    ids.sort();
    ids.dedup();
    let found: i64 = chapters::table
        .filter(chapters::work_id.eq(work_id))
        .filter(chapters::id.eq_any(&ids))
        .count()
        .get_result(conn)?;
    if found as usize != ids.len() {
        return Err(ApiError::BadRequest(
            "every chapter has to belong to this work".into(),
        ));
    }
    Ok(Some(ids))
}

fn grant(conn: &mut Conn, collaborator_id: &str, chapter_ids: &[String]) -> ApiResult<()> {
    for chapter_id in chapter_ids {
        diesel::insert_into(collaborator_chapters::table)
            .values(&NewCollaboratorChapter {
                id: new_id(),
                collaborator_id: collaborator_id.to_string(),
                chapter_id: chapter_id.clone(),
            })
            .execute(conn)?;
    }
    Ok(())
}

// --- activity ------------------------------------------------------------

/// How long a run of saves to one scene by one person stays one line.
const EDIT_SITTING_MINUTES: i64 = 30;

/// Notes that something happened to a work.
///
/// Edits fold together: the same person saving the same scene again within
/// half an hour updates the line already there rather than adding one, since
/// autosave fires every couple of seconds and a feed of two hundred "edited
/// scene three" lines says less than one.
pub fn record(
    conn: &mut Conn,
    work_id: &str,
    actor_id: &str,
    kind: ActivityKind,
    subject_id: Option<&str>,
    subject: &str,
) -> ApiResult<()> {
    let now = now_iso();
    if kind == ActivityKind::SceneEdited {
        let since = (chrono::Utc::now() - chrono::Duration::minutes(EDIT_SITTING_MINUTES))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let recent: Option<String> = work_activity::table
            .filter(work_activity::work_id.eq(work_id))
            .filter(work_activity::actor_id.eq(actor_id))
            .filter(work_activity::kind.eq(kind.as_str()))
            .filter(work_activity::subject_id.eq(subject_id))
            .filter(work_activity::updated_at.ge(since))
            .order(work_activity::updated_at.desc())
            .select(work_activity::id)
            .first(conn)
            .optional()?;
        if let Some(id) = recent {
            diesel::update(work_activity::table.find(&id))
                .set((
                    work_activity::updated_at.eq(&now),
                    work_activity::times.eq(work_activity::times + 1),
                    work_activity::subject.eq(subject),
                ))
                .execute(conn)?;
            return Ok(());
        }
    }
    diesel::insert_into(work_activity::table)
        .values(&NewWorkActivity {
            id: new_id(),
            work_id: work_id.to_string(),
            actor_id: actor_id.to_string(),
            kind: kind.as_str().to_string(),
            subject_id: subject_id.map(str::to_string),
            subject: subject.to_string(),
            times: 1,
            created_at: now.clone(),
            updated_at: now,
        })
        .execute(conn)?;
    Ok(())
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/activity",
    tag = "collaboration",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "What has happened to this work, newest first", body = Vec<ActivityView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn activity(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Query(query): Query<ActivityQuery>,
) -> ApiResult<Json<Vec<ActivityView>>> {
    let (limit, offset) = Page {
        limit: query.limit.unwrap_or(50),
        offset: query.offset.unwrap_or(0),
    }
    .clamped();
    let views = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Read)?;
        let rows: Vec<WorkActivity> = work_activity::table
            .filter(work_activity::work_id.eq(&work_id))
            .order(work_activity::updated_at.desc())
            .limit(limit)
            .offset(offset)
            .select(WorkActivity::as_select())
            .load(conn)?;
        let ids: Vec<String> = rows.iter().map(|r| r.actor_id.clone()).collect();
        let cards = support::author_cards(conn, &ids)?;
        Ok(rows
            .into_iter()
            .map(|activity| ActivityView {
                actor: cards
                    .get(&activity.actor_id)
                    .cloned()
                    .unwrap_or(AuthorCard {
                        id: activity.actor_id.clone(),
                        display_name: "Unknown".into(),
                    }),
                activity,
            })
            .collect())
    })
    .await?;
    Ok(Json(views))
}
