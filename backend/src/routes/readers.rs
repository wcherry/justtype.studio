//! Who a manuscript was shared with, and how far they got.
//!
//! Invitations are by email rather than by account: the person a writer wants
//! to read their novel usually has an email address and not yet an account
//! here. Matching happens when that address signs in, so an invitation sent
//! before the reader ever heard of this application still works.
use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{circle_members, publication_invites, reader_circles, reading_progress};
use crate::domain::enums::AuditAction;
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support;

pub(super) fn normalise(email: &str) -> ApiResult<String> {
    let email = email.trim().to_lowercase();
    if !email.contains('@') || email.len() < 5 {
        return Err(ApiError::BadRequest(format!(
            "`{email}` does not look like an email address"
        )));
    }
    Ok(email)
}

// --- invitations ---------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InviteRequest {
    /// Addresses to invite. Duplicates and ones already invited are ignored.
    #[serde(default)]
    pub emails: Vec<String>,
    /// Invite everyone in one of the author's circles, by id.
    #[serde(default)]
    pub circle_id: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InviteView {
    #[serde(flatten)]
    pub invite: PublicationInvite,
    /// Whether that reader has opened it yet.
    pub accepted: bool,
}

impl InviteView {
    /// An account picked by name is shown without its address: the author
    /// chose a person, and their email is theirs.
    pub fn of(mut invite: PublicationInvite) -> InviteView {
        if invite.user_id.is_some() {
            invite.email.clear();
        }
        InviteView {
            accepted: invite.accepted_at.is_some(),
            invite,
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/publications/{publication_id}/invites",
    tag = "readers",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "Who it was shared with", body = Vec<InviteView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_invites(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
) -> ApiResult<Json<Vec<InviteView>>> {
    let invites = blocking(&state.pool, move |conn| {
        support::publication_for_owner(conn, &publication_id, &user)?;
        let rows: Vec<PublicationInvite> = publication_invites::table
            .filter(publication_invites::publication_id.eq(&publication_id))
            .order(publication_invites::email.asc())
            .select(PublicationInvite::as_select())
            .load(conn)?;
        Ok(rows.into_iter().map(InviteView::of).collect())
    })
    .await?;
    Ok(Json(invites))
}

#[utoipa::path(
    post,
    path = "/api/publications/{publication_id}/invites",
    tag = "readers",
    params(("publication_id" = String, Path)),
    request_body = InviteRequest,
    responses(
        (status = 200, description = "Who it is now shared with", body = Vec<InviteView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn invite(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
    Json(body): Json<InviteRequest>,
) -> ApiResult<Json<Vec<InviteView>>> {
    let mut emails: Vec<String> = Vec::new();
    for email in &body.emails {
        emails.push(normalise(email)?);
    }

    let invites = blocking(&state.pool, move |conn| {
        let (_, work) = support::publication_for_owner(conn, &publication_id, &user)?;

        if let Some(circle_id) = &body.circle_id {
            let circle = circle_for_owner(conn, circle_id, &user)?;
            let members: Vec<String> = circle_members::table
                .filter(circle_members::circle_id.eq(&circle.id))
                .select(circle_members::email)
                .load(conn)?;
            emails.extend(members);
        }
        emails.sort();
        emails.dedup();
        if emails.is_empty() {
            return Err(ApiError::BadRequest("nobody to invite".into()));
        }

        let now = now_iso();
        for email in &emails {
            diesel::insert_into(publication_invites::table)
                .values(&NewPublicationInvite {
                    id: new_id(),
                    publication_id: publication_id.clone(),
                    email: email.clone(),
                    invited_by: user.id.clone(),
                    accepted_at: None,
                    accepted_by: None,
                    created_at: now.clone(),
                    user_id: None,
                })
                // Inviting the same person twice is what someone does when they
                // cannot remember whether they already did.
                .on_conflict((
                    publication_invites::publication_id,
                    publication_invites::email,
                ))
                .do_nothing()
                .execute(conn)?;
        }
        super::audit::record(
            conn,
            Some(&user.id),
            Some(&work.id),
            AuditAction::ReaderInvited,
            &format!("{publication_id}: {}", emails.join(", ")),
        )?;

        let rows: Vec<PublicationInvite> = publication_invites::table
            .filter(publication_invites::publication_id.eq(&publication_id))
            .order(publication_invites::email.asc())
            .select(PublicationInvite::as_select())
            .load(conn)?;
        Ok(rows.into_iter().map(InviteView::of).collect())
    })
    .await?;
    Ok(Json(invites))
}

#[utoipa::path(
    delete,
    path = "/api/publications/{publication_id}/invites/{email}",
    tag = "readers",
    params(("publication_id" = String, Path), ("email" = String, Path)),
    responses(
        (status = 200, description = "Removed"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn uninvite(
    State(state): State<AppState>,
    user: AuthUser,
    Path((publication_id, email)): Path<(String, String)>,
) -> ApiResult<Json<serde_json::Value>> {
    let email = normalise(&email)?;
    blocking(&state.pool, move |conn| {
        let (_, work) = support::publication_for_owner(conn, &publication_id, &user)?;
        diesel::delete(
            publication_invites::table
                .filter(publication_invites::publication_id.eq(&publication_id))
                .filter(publication_invites::email.eq(&email)),
        )
        .execute(conn)?;
        super::audit::record(
            conn,
            Some(&user.id),
            Some(&work.id),
            AuditAction::ReaderUninvited,
            &format!("{publication_id}: {email}"),
        )?;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- circles -------------------------------------------------------------

fn circle_for_owner(
    conn: &mut crate::db::Conn,
    circle_id: &str,
    user: &AuthUser,
) -> ApiResult<ReaderCircle> {
    let circle: ReaderCircle = reader_circles::table
        .find(circle_id)
        .select(ReaderCircle::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("circle not found".into()))?;
    if circle.owner_id != user.id {
        return Err(ApiError::Forbidden("that is not your circle".into()));
    }
    Ok(circle)
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CircleView {
    #[serde(flatten)]
    pub circle: ReaderCircle,
    pub members: Vec<CircleMember>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCircleRequest {
    pub name: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddMemberRequest {
    pub email: String,
    /// What to call them, since an address is a poor way to recognise a friend.
    #[serde(default)]
    pub name: Option<String>,
}

fn load_circle(conn: &mut crate::db::Conn, circle_id: &str) -> ApiResult<CircleView> {
    let circle: ReaderCircle = reader_circles::table
        .find(circle_id)
        .select(ReaderCircle::as_select())
        .first(conn)?;
    let members: Vec<CircleMember> = circle_members::table
        .filter(circle_members::circle_id.eq(circle_id))
        .order(circle_members::email.asc())
        .select(CircleMember::as_select())
        .load(conn)?;
    Ok(CircleView { circle, members })
}

#[utoipa::path(
    get,
    path = "/api/circles",
    tag = "readers",
    responses(
        (status = 200, description = "The writer's circles", body = Vec<CircleView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_circles(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<CircleView>>> {
    let circles = blocking(&state.pool, move |conn| {
        let ids: Vec<String> = reader_circles::table
            .filter(reader_circles::owner_id.eq(&user.id))
            .order(reader_circles::name.asc())
            .select(reader_circles::id)
            .load(conn)?;
        ids.iter().map(|id| load_circle(conn, id)).collect()
    })
    .await?;
    Ok(Json(circles))
}

#[utoipa::path(
    post,
    path = "/api/circles",
    tag = "readers",
    request_body = CreateCircleRequest,
    responses(
        (status = 200, description = "The new circle", body = CircleView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create_circle(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<CreateCircleRequest>,
) -> ApiResult<Json<CircleView>> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::BadRequest("a circle needs a name".into()));
    }
    let circle = blocking(&state.pool, move |conn| {
        let now = now_iso();
        let row = NewReaderCircle {
            id: new_id(),
            owner_id: user.id.clone(),
            name,
            created_at: now.clone(),
            updated_at: now,
        };
        diesel::insert_into(reader_circles::table)
            .values(&row)
            .execute(conn)?;
        load_circle(conn, &row.id)
    })
    .await?;
    Ok(Json(circle))
}

#[utoipa::path(
    post,
    path = "/api/circles/{circle_id}/members",
    tag = "readers",
    params(("circle_id" = String, Path)),
    request_body = AddMemberRequest,
    responses(
        (status = 200, description = "The circle", body = CircleView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn add_member(
    State(state): State<AppState>,
    user: AuthUser,
    Path(circle_id): Path<String>,
    Json(body): Json<AddMemberRequest>,
) -> ApiResult<Json<CircleView>> {
    let email = normalise(&body.email)?;
    let circle = blocking(&state.pool, move |conn| {
        circle_for_owner(conn, &circle_id, &user)?;
        diesel::insert_into(circle_members::table)
            .values(&NewCircleMember {
                id: new_id(),
                circle_id: circle_id.clone(),
                email,
                name: body.name.clone(),
                created_at: now_iso(),
            })
            .on_conflict((circle_members::circle_id, circle_members::email))
            .do_nothing()
            .execute(conn)?;
        load_circle(conn, &circle_id)
    })
    .await?;
    Ok(Json(circle))
}

#[utoipa::path(
    delete,
    path = "/api/circles/{circle_id}/members/{email}",
    tag = "readers",
    params(("circle_id" = String, Path), ("email" = String, Path)),
    responses(
        (status = 200, description = "The circle", body = CircleView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn remove_member(
    State(state): State<AppState>,
    user: AuthUser,
    Path((circle_id, email)): Path<(String, String)>,
) -> ApiResult<Json<CircleView>> {
    let email = normalise(&email)?;
    let circle = blocking(&state.pool, move |conn| {
        circle_for_owner(conn, &circle_id, &user)?;
        diesel::delete(
            circle_members::table
                .filter(circle_members::circle_id.eq(&circle_id))
                .filter(circle_members::email.eq(&email)),
        )
        .execute(conn)?;
        load_circle(conn, &circle_id)
    })
    .await?;
    Ok(Json(circle))
}

#[utoipa::path(
    delete,
    path = "/api/circles/{circle_id}",
    tag = "readers",
    params(("circle_id" = String, Path)),
    responses(
        (status = 200, description = "Removed"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_circle(
    State(state): State<AppState>,
    user: AuthUser,
    Path(circle_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    blocking(&state.pool, move |conn| {
        circle_for_owner(conn, &circle_id, &user)?;
        diesel::delete(reader_circles::table.find(&circle_id)).execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// --- reading progress ----------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProgressRequest {
    /// The scene the reader is looking at.
    pub scene_id: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReaderProgressView {
    #[serde(flatten)]
    pub progress: ReadingProgress,
    pub reader: support::AuthorCard,
    /// Furthest point reached, as a percentage of the published scenes.
    pub percent: i32,
}

/// Records where a reader has got to.
///
/// Only ever moves the furthest mark forward: a reader who gave up in chapter
/// three and later skimmed the ending has still stopped in chapter three, and
/// that is the fact an author needs.
#[utoipa::path(
    put,
    path = "/api/publications/{publication_id}/progress",
    tag = "readers",
    params(("publication_id" = String, Path)),
    request_body = ProgressRequest,
    responses(
        (status = 200, description = "Where they are", body = ReaderProgressView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn record_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
    Json(body): Json<ProgressRequest>,
) -> ApiResult<Json<ReaderProgressView>> {
    use crate::db::schema::version_entries;

    let view = blocking(&state.pool, move |conn| {
        let (publication, _) = support::publication_for_reader(conn, &publication_id, Some(&user))?;
        // Opening it counts as turning up.
        support::mark_invite_accepted(conn, &publication.id, &user)?;

        let entries: Vec<String> = version_entries::table
            .filter(version_entries::version_id.eq(&publication.version_id))
            .order((
                version_entries::chapter_position.asc(),
                version_entries::scene_position.asc(),
            ))
            .select(version_entries::scene_id)
            .load(conn)?;

        let index = entries
            .iter()
            .position(|id| id == &body.scene_id)
            .ok_or_else(|| ApiError::BadRequest("that scene is not in this publication".into()))?
            as i32;
        let total = entries.len() as i32;
        let now = now_iso();

        let existing: Option<ReadingProgress> = reading_progress::table
            .filter(reading_progress::publication_id.eq(&publication.id))
            .filter(reading_progress::user_id.eq(&user.id))
            .select(ReadingProgress::as_select())
            .first(conn)
            .optional()?;

        match existing {
            Some(row) => {
                let furthest = row.furthest_index.max(index);
                diesel::update(reading_progress::table.find(&row.id))
                    .set((
                        reading_progress::scene_id.eq(&body.scene_id),
                        reading_progress::furthest_index.eq(furthest),
                        reading_progress::scenes_total.eq(total),
                        reading_progress::finished_at.eq(if furthest + 1 >= total {
                            Some(row.finished_at.unwrap_or_else(|| now.clone()))
                        } else {
                            None
                        }),
                        reading_progress::updated_at.eq(&now),
                    ))
                    .execute(conn)?;
            }
            None => {
                diesel::insert_into(reading_progress::table)
                    .values(&NewReadingProgress {
                        id: new_id(),
                        publication_id: publication.id.clone(),
                        user_id: user.id.clone(),
                        scene_id: body.scene_id.clone(),
                        furthest_index: index,
                        scenes_total: total,
                        finished_at: (index + 1 >= total).then(|| now.clone()),
                        created_at: now.clone(),
                        updated_at: now.clone(),
                    })
                    .execute(conn)?;
            }
        }

        let progress: ReadingProgress = reading_progress::table
            .filter(reading_progress::publication_id.eq(&publication.id))
            .filter(reading_progress::user_id.eq(&user.id))
            .select(ReadingProgress::as_select())
            .first(conn)?;
        let cards = support::author_cards(conn, std::slice::from_ref(&user.id))?;
        Ok(view_of(progress, &cards))
    })
    .await?;
    Ok(Json(view))
}

fn view_of(
    progress: ReadingProgress,
    cards: &std::collections::HashMap<String, support::AuthorCard>,
) -> ReaderProgressView {
    let percent = if progress.scenes_total > 0 {
        ((progress.furthest_index + 1) * 100) / progress.scenes_total
    } else {
        0
    };
    ReaderProgressView {
        reader: cards
            .get(&progress.user_id)
            .cloned()
            .unwrap_or_else(|| support::AuthorCard {
                id: progress.user_id.clone(),
                display_name: "Unknown".into(),
            }),
        percent,
        progress,
    }
}

/// How far everyone got — the author's view of a review in progress.
#[utoipa::path(
    get,
    path = "/api/publications/{publication_id}/progress",
    tag = "readers",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "Every reader's progress", body = Vec<ReaderProgressView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_progress(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
) -> ApiResult<Json<Vec<ReaderProgressView>>> {
    let views = blocking(&state.pool, move |conn| {
        support::publication_for_owner(conn, &publication_id, &user)?;
        let rows: Vec<ReadingProgress> = reading_progress::table
            .filter(reading_progress::publication_id.eq(&publication_id))
            .order(reading_progress::furthest_index.desc())
            .select(ReadingProgress::as_select())
            .load(conn)?;
        let ids: Vec<String> = rows.iter().map(|r| r.user_id.clone()).collect();
        let cards = support::author_cards(conn, &ids)?;
        Ok(rows.into_iter().map(|row| view_of(row, &cards)).collect())
    })
    .await?;
    Ok(Json(views))
}
