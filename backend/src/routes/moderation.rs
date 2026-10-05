//! Reporting a comment, and an author's own list of people who do not get to
//! read their work.
//!
//! There is no moderation team, so a report goes to the only person with both
//! standing and motive to act on it: the author whose work the comment is
//! attached to. They can already delete a comment on their own publication;
//! what a report adds is that a *third* reader can say something is wrong
//! without having to email anybody.
use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{blocked_readers, comment_reports, comments, users};
use crate::domain::enums::{AuditAction, ReportReason};
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, AuthorCard};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReportRequest {
    #[schema(value_type = crate::domain::enums::ReportReason)]
    pub reason: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReportView {
    #[serde(flatten)]
    pub report: CommentReport,
    pub reporter: AuthorCard,
    /// What was said, so the author can judge without going to find it.
    pub comment_body: String,
    pub comment_author: AuthorCard,
    pub publication_id: String,
}

#[utoipa::path(
    post,
    path = "/api/comments/{comment_id}/report",
    tag = "moderation",
    params(("comment_id" = String, Path)),
    request_body = ReportRequest,
    responses(
        (status = 200, description = "Reported", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn report(
    State(state): State<AppState>,
    user: AuthUser,
    Path(comment_id): Path<String>,
    Json(body): Json<ReportRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    if !ReportReason::accepts(&body.reason) {
        return Err(ApiError::BadRequest(format!(
            "reason must be one of {}",
            ReportReason::options()
        )));
    }

    blocking(&state.pool, move |conn| {
        let comment: Comment = comments::table
            .find(&comment_id)
            .filter(comments::state.ne("deleted"))
            .select(Comment::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("comment not found".into()))?;
        // You have to be able to see it to report it.
        support::publication_for_reader(conn, &comment.publication_id, Some(&user))?;
        if comment.author_id == user.id {
            return Err(ApiError::BadRequest(
                "reporting your own comment does nothing — delete it instead".into(),
            ));
        }

        // Reporting the same comment twice is the same report, not a louder
        // one; the second one updates the reason rather than stacking.
        let existing: Option<String> = comment_reports::table
            .filter(comment_reports::comment_id.eq(&comment.id))
            .filter(comment_reports::reporter_id.eq(&user.id))
            .select(comment_reports::id)
            .first(conn)
            .optional()?;
        match existing {
            Some(id) => {
                diesel::update(comment_reports::table.find(&id))
                    .set((
                        comment_reports::reason.eq(&body.reason),
                        comment_reports::detail.eq(body.detail.clone()),
                    ))
                    .execute(conn)?;
            }
            None => {
                diesel::insert_into(comment_reports::table)
                    .values(&NewCommentReport {
                        id: new_id(),
                        comment_id: comment.id.clone(),
                        reporter_id: user.id.clone(),
                        reason: body.reason.clone(),
                        detail: body.detail.clone(),
                        created_at: now_iso(),
                    })
                    .execute(conn)?;
            }
        }
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "reported": true })))
}

#[utoipa::path(
    get,
    path = "/api/publications/{publication_id}/reports",
    tag = "moderation",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "What readers flagged", body = Vec<ReportView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_reports(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
) -> ApiResult<Json<Vec<ReportView>>> {
    let views = blocking(&state.pool, move |conn| {
        support::publication_for_owner(conn, &publication_id, &user)?;
        let rows: Vec<(CommentReport, Comment)> = comment_reports::table
            .inner_join(comments::table.on(comments::id.eq(comment_reports::comment_id)))
            .filter(comments::publication_id.eq(&publication_id))
            .filter(comments::state.ne("deleted"))
            .order(comment_reports::created_at.desc())
            .select((CommentReport::as_select(), Comment::as_select()))
            .load(conn)?;

        let ids: Vec<String> = rows
            .iter()
            .flat_map(|(r, c)| [r.reporter_id.clone(), c.author_id.clone()])
            .collect();
        let cards = support::author_cards(conn, &ids)?;
        let unknown = |id: &str| AuthorCard {
            id: id.to_string(),
            display_name: "Unknown".into(),
        };

        Ok(rows
            .into_iter()
            .map(|(report, comment)| ReportView {
                reporter: cards
                    .get(&report.reporter_id)
                    .cloned()
                    .unwrap_or_else(|| unknown(&report.reporter_id)),
                comment_author: cards
                    .get(&comment.author_id)
                    .cloned()
                    .unwrap_or_else(|| unknown(&comment.author_id)),
                comment_body: comment.body,
                publication_id: comment.publication_id,
                report,
            })
            .collect())
    })
    .await?;
    Ok(Json(views))
}

// --- the author's door ---------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockRequest {
    /// Who to shut out. An id rather than an address: this is somebody who has
    /// already turned up.
    pub user_id: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BlockView {
    #[serde(flatten)]
    pub block: BlockedReader,
    pub reader: AuthorCard,
}

#[utoipa::path(
    get,
    path = "/api/me/blocks",
    tag = "moderation",
    responses(
        (status = 200, description = "Who cannot read your work", body = Vec<BlockView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_blocks(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Vec<BlockView>>> {
    let views = blocking(&state.pool, move |conn| {
        let rows: Vec<BlockedReader> = blocked_readers::table
            .filter(blocked_readers::owner_id.eq(&user.id))
            .order(blocked_readers::created_at.desc())
            .select(BlockedReader::as_select())
            .load(conn)?;
        let ids: Vec<String> = rows.iter().map(|b| b.user_id.clone()).collect();
        let cards = support::author_cards(conn, &ids)?;
        Ok(rows
            .into_iter()
            .map(|block| BlockView {
                reader: cards.get(&block.user_id).cloned().unwrap_or(AuthorCard {
                    id: block.user_id.clone(),
                    display_name: "Unknown".into(),
                }),
                block,
            })
            .collect())
    })
    .await?;
    Ok(Json(views))
}

#[utoipa::path(
    post,
    path = "/api/me/blocks",
    tag = "moderation",
    request_body = BlockRequest,
    responses(
        (status = 200, description = "Blocked", body = Vec<BlockView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn block(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<BlockRequest>,
) -> ApiResult<Json<Vec<BlockView>>> {
    blocking(&state.pool, {
        let user = user.clone();
        move |conn| {
            if body.user_id == user.id {
                return Err(ApiError::BadRequest("you cannot block yourself".into()));
            }
            let exists: i64 = users::table.find(&body.user_id).count().get_result(conn)?;
            if exists == 0 {
                return Err(ApiError::NotFound("no such reader".into()));
            }
            let already: i64 = blocked_readers::table
                .filter(blocked_readers::owner_id.eq(&user.id))
                .filter(blocked_readers::user_id.eq(&body.user_id))
                .count()
                .get_result(conn)?;
            if already == 0 {
                diesel::insert_into(blocked_readers::table)
                    .values(&NewBlockedReader {
                        id: new_id(),
                        owner_id: user.id.clone(),
                        user_id: body.user_id.clone(),
                        created_at: now_iso(),
                    })
                    .execute(conn)?;
                super::audit::record(
                    conn,
                    Some(&user.id),
                    None,
                    AuditAction::ReaderBlocked,
                    &body.user_id,
                )?;
            }
            Ok(())
        }
    })
    .await?;
    list_blocks(State(state), user).await
}

#[utoipa::path(
    delete,
    path = "/api/me/blocks/{user_id}",
    tag = "moderation",
    params(("user_id" = String, Path)),
    responses(
        (status = 200, description = "Let back in", body = Vec<BlockView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn unblock(
    State(state): State<AppState>,
    user: AuthUser,
    Path(user_id): Path<String>,
) -> ApiResult<Json<Vec<BlockView>>> {
    blocking(&state.pool, {
        let user = user.clone();
        move |conn| {
            diesel::delete(
                blocked_readers::table
                    .filter(blocked_readers::owner_id.eq(&user.id))
                    .filter(blocked_readers::user_id.eq(&user_id)),
            )
            .execute(conn)?;
            super::audit::record(
                conn,
                Some(&user.id),
                None,
                AuditAction::ReaderUnblocked,
                &user_id,
            )?;
            Ok(())
        }
    })
    .await?;
    list_blocks(State(state), user).await
}
