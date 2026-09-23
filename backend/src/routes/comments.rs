//! Reviewer feedback: anchored comments, threaded replies, and voting.
//!
//! An anchor is a `{from, to}` position pair into the snapshot document plus the
//! quoted text. The snapshot is immutable, so an anchor can never drift — that
//! is the main reason comments hang off a version rather than the working copy.
use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::auth::{AuthUser, MaybeAuthUser};
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{comment_votes, comments};
use crate::domain::enums::CommentCategory;
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, AuthorCard};

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListQuery {
    /// Narrow to one scene, for the reader's sidebar.
    #[serde(default)]
    pub scene_id: Option<String>,
    /// `top` (default) or `new`.
    #[serde(default)]
    pub sort: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    pub from: i32,
    pub to: i32,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateCommentRequest {
    pub body: String,
    #[serde(default)]
    pub scene_id: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Present for highlight-to-comment; absent for a general note.
    #[serde(default)]
    pub anchor: Option<Anchor>,
    #[serde(default)]
    pub anchor_quote: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::CommentCategory>)]
    pub category: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCommentRequest {
    #[serde(default)]
    pub body: Option<String>,
    /// `open` or `resolved`; only the author of the work may resolve.
    #[serde(default)]
    pub state: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VoteRequest {
    /// `1` upvote, `-1` downvote, `0` clears the caller's vote.
    pub value: i32,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CommentView {
    #[serde(flatten)]
    pub comment: Comment,
    pub author: AuthorCard,
    pub viewer_vote: i32,
    /// Threads nest arbitrarily deep, which makes this type self-referential.
    /// Schema generation has to be told to stop, or it recurses until the stack
    /// gives out.
    #[schema(no_recursion)]
    pub replies: Vec<CommentView>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct VoteResponse {
    pub comment_id: String,
    pub score: i32,
    pub viewer_vote: i32,
}

#[utoipa::path(
    get,
    path = "/api/publications/{publication_id}/comments",
    tag = "comments",
    params(("publication_id" = String, Path)),
    responses(
        (status = 200, description = "Success", body = Vec<CommentView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    MaybeAuthUser(user): MaybeAuthUser,
    Path(publication_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Vec<CommentView>>> {
    let viewer_id = user.as_ref().map(|u| u.id.clone());
    let sort_by_score = query.sort.as_deref() != Some("new");

    let views = blocking(&state.pool, move |conn| {
        support::publication_for_reader(conn, &publication_id, user.as_ref())?;

        let mut q = comments::table
            .filter(comments::publication_id.eq(&publication_id))
            .filter(comments::state.ne("deleted"))
            .into_boxed();
        if let Some(scene_id) = &query.scene_id {
            q = q.filter(comments::scene_id.eq(scene_id.clone()));
        }
        let rows: Vec<Comment> = q
            .order(comments::created_at.asc())
            .select(Comment::as_select())
            .load(conn)?;

        let author_ids: Vec<String> = rows.iter().map(|c| c.author_id.clone()).collect();
        let authors = support::author_cards(conn, &author_ids)?;

        let votes: HashMap<String, i32> = match &viewer_id {
            Some(id) => comment_votes::table
                .filter(comment_votes::user_id.eq(id))
                .select((comment_votes::comment_id, comment_votes::value))
                .load::<(String, i32)>(conn)?
                .into_iter()
                .collect(),
            None => HashMap::new(),
        };

        Ok(build_threads(rows, &authors, &votes, sort_by_score))
    })
    .await?;
    Ok(Json(views))
}

/// Flat rows in, nested threads out. Replies always read oldest-first; roots
/// respect the requested sort.
fn build_threads(
    rows: Vec<Comment>,
    authors: &HashMap<String, AuthorCard>,
    votes: &HashMap<String, i32>,
    sort_by_score: bool,
) -> Vec<CommentView> {
    let mut children: HashMap<String, Vec<Comment>> = HashMap::new();
    let mut roots: Vec<Comment> = Vec::new();
    for row in rows {
        match row.parent_id.clone() {
            Some(parent) => children.entry(parent).or_default().push(row),
            None => roots.push(row),
        }
    }
    if sort_by_score {
        roots.sort_by(|a, b| b.score.cmp(&a.score).then(a.created_at.cmp(&b.created_at)));
    }

    fn to_view(
        comment: Comment,
        children: &mut HashMap<String, Vec<Comment>>,
        authors: &HashMap<String, AuthorCard>,
        votes: &HashMap<String, i32>,
    ) -> CommentView {
        let replies = children
            .remove(&comment.id)
            .unwrap_or_default()
            .into_iter()
            .map(|child| to_view(child, children, authors, votes))
            .collect();
        CommentView {
            author: authors
                .get(&comment.author_id)
                .cloned()
                .unwrap_or_else(|| AuthorCard {
                    id: comment.author_id.clone(),
                    display_name: "Unknown".into(),
                }),
            viewer_vote: votes.get(&comment.id).copied().unwrap_or(0),
            replies,
            comment,
        }
    }

    roots
        .into_iter()
        .map(|root| to_view(root, &mut children, authors, votes))
        .collect()
}

#[utoipa::path(
    post,
    path = "/api/publications/{publication_id}/comments",
    tag = "comments",
    params(("publication_id" = String, Path)),
    request_body = CreateCommentRequest,
    responses(
        (status = 200, description = "Success", body = CommentView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
    Json(body): Json<CreateCommentRequest>,
) -> ApiResult<Json<CommentView>> {
    let text = body.body.trim().to_string();
    if text.is_empty() {
        return Err(ApiError::BadRequest("a comment needs a body".into()));
    }
    let category = body.category.clone().unwrap_or_else(|| "general".into());
    if !CommentCategory::accepts(&category) {
        return Err(ApiError::BadRequest(format!(
            "category must be one of {}",
            CommentCategory::options()
        )));
    }

    let view = blocking(&state.pool, move |conn| {
        let (publication, _) = support::publication_for_reader(conn, &publication_id, Some(&user))?;
        // The window is enforced by `publication_for_reader` above; the cap is
        // about who may speak rather than who may read, so it belongs here.
        support::within_reviewer_cap(conn, &publication, &user)?;
        // A reply must belong to the same publication as its parent.
        if let Some(parent_id) = &body.parent_id {
            let parent: Comment = comments::table
                .find(parent_id)
                .select(Comment::as_select())
                .first(conn)
                .optional()?
                .ok_or_else(|| ApiError::NotFound("parent comment not found".into()))?;
            if parent.publication_id != publication.id {
                return Err(ApiError::BadRequest(
                    "cannot reply across publications".into(),
                ));
            }
        }

        let now = now_iso();
        let row = NewComment {
            id: new_id(),
            publication_id: publication.id.clone(),
            version_id: publication.version_id.clone(),
            scene_id: body.scene_id.clone(),
            parent_id: body.parent_id.clone(),
            author_id: user.id.clone(),
            body: text,
            anchor: body
                .anchor
                .as_ref()
                .map(|a| serde_json::json!({ "from": a.from, "to": a.to }).to_string()),
            anchor_quote: body.anchor_quote.clone(),
            category,
            state: "open".into(),
            score: 0,
            created_at: now.clone(),
            updated_at: now,
        };
        diesel::insert_into(comments::table)
            .values(&row)
            .execute(conn)?;

        let comment: Comment = comments::table
            .find(&row.id)
            .select(Comment::as_select())
            .first(conn)?;
        let authors = support::author_cards(conn, std::slice::from_ref(&comment.author_id))?;
        Ok(CommentView {
            author: authors
                .get(&comment.author_id)
                .cloned()
                .unwrap_or_else(|| AuthorCard {
                    id: comment.author_id.clone(),
                    display_name: "Unknown".into(),
                }),
            viewer_vote: 0,
            replies: Vec::new(),
            comment,
        })
    })
    .await?;
    Ok(Json(view))
}

#[utoipa::path(
    patch,
    path = "/api/comments/{comment_id}",
    tag = "comments",
    params(("comment_id" = String, Path)),
    request_body = UpdateCommentRequest,
    responses(
        (status = 200, description = "Success", body = Comment),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(comment_id): Path<String>,
    Json(body): Json<UpdateCommentRequest>,
) -> ApiResult<Json<Comment>> {
    let comment = blocking(&state.pool, move |conn| {
        let comment: Comment = comments::table
            .find(&comment_id)
            .select(Comment::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("comment not found".into()))?;
        let now = now_iso();

        if let Some(text) = &body.body {
            if comment.author_id != user.id {
                return Err(ApiError::Forbidden(
                    "only the author can edit a comment".into(),
                ));
            }
            if text.trim().is_empty() {
                return Err(ApiError::BadRequest("a comment needs a body".into()));
            }
            diesel::update(comments::table.find(&comment.id))
                .set(comments::body.eq(text.trim()))
                .execute(conn)?;
        }
        if let Some(new_state) = &body.state {
            if new_state != "open" && new_state != "resolved" {
                return Err(ApiError::BadRequest(
                    "state must be `open` or `resolved`".into(),
                ));
            }
            // Resolving is the manuscript author's call, not the reviewer's.
            support::publication_for_owner(conn, &comment.publication_id, &user)?;
            diesel::update(comments::table.find(&comment.id))
                .set(comments::state.eq(new_state))
                .execute(conn)?;
        }
        diesel::update(comments::table.find(&comment.id))
            .set(comments::updated_at.eq(&now))
            .execute(conn)?;
        Ok(comments::table
            .find(&comment.id)
            .select(Comment::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(comment))
}

/// Soft delete: threads keep their shape, and the work's author can remove
/// abusive feedback from their own publication.
#[utoipa::path(
    delete,
    path = "/api/comments/{comment_id}",
    tag = "comments",
    params(("comment_id" = String, Path)),
    responses(
        (status = 200, description = "Success"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_comment(
    State(state): State<AppState>,
    user: AuthUser,
    Path(comment_id): Path<String>,
) -> ApiResult<Json<Value>> {
    blocking(&state.pool, move |conn| {
        let comment: Comment = comments::table
            .find(&comment_id)
            .select(Comment::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("comment not found".into()))?;
        let is_author = comment.author_id == user.id;
        let is_work_owner =
            support::publication_for_owner(conn, &comment.publication_id, &user).is_ok();
        if !is_author && !is_work_owner {
            return Err(ApiError::Forbidden("you cannot delete this comment".into()));
        }
        diesel::update(comments::table.find(&comment.id))
            .set((
                comments::state.eq("deleted"),
                comments::body.eq(""),
                comments::updated_at.eq(now_iso()),
            ))
            .execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Votes surface the feedback other reviewers agree with. One vote per person
/// per comment; sending the same value again clears it.
#[utoipa::path(
    post,
    path = "/api/comments/{comment_id}/vote",
    tag = "comments",
    params(("comment_id" = String, Path)),
    request_body = VoteRequest,
    responses(
        (status = 200, description = "Success", body = VoteResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn vote(
    State(state): State<AppState>,
    user: AuthUser,
    Path(comment_id): Path<String>,
    Json(body): Json<VoteRequest>,
) -> ApiResult<Json<VoteResponse>> {
    if ![-1, 0, 1].contains(&body.value) {
        return Err(ApiError::BadRequest("value must be -1, 0 or 1".into()));
    }

    let response = blocking(&state.pool, move |conn| {
        let comment: Comment = comments::table
            .find(&comment_id)
            .filter(comments::state.ne("deleted"))
            .select(Comment::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("comment not found".into()))?;
        support::publication_for_reader(conn, &comment.publication_id, Some(&user))?;
        if comment.author_id == user.id {
            return Err(ApiError::Forbidden(
                "you cannot vote on your own comment".into(),
            ));
        }

        let existing: Option<CommentVote> = comment_votes::table
            .filter(comment_votes::comment_id.eq(&comment.id))
            .filter(comment_votes::user_id.eq(&user.id))
            .select(CommentVote::as_select())
            .first(conn)
            .optional()?;

        let viewer_vote = conn.transaction::<_, ApiError, _>(|conn| {
            let target = match (&existing, body.value) {
                (_, 0) => 0,
                (Some(prev), value) if prev.value == value => 0,
                (_, value) => value,
            };
            if let Some(prev) = &existing {
                diesel::delete(comment_votes::table.find(&prev.id)).execute(conn)?;
            }
            if target != 0 {
                diesel::insert_into(comment_votes::table)
                    .values(&NewCommentVote {
                        id: new_id(),
                        comment_id: comment.id.clone(),
                        user_id: user.id.clone(),
                        value: target,
                        created_at: now_iso(),
                    })
                    .execute(conn)?;
            }
            // Recompute rather than increment, so the cached score cannot drift.
            let score: i64 = comment_votes::table
                .filter(comment_votes::comment_id.eq(&comment.id))
                .select(diesel::dsl::sum(comment_votes::value))
                .first::<Option<i64>>(conn)?
                .unwrap_or(0);
            diesel::update(comments::table.find(&comment.id))
                .set(comments::score.eq(score as i32))
                .execute(conn)?;
            Ok(target)
        })?;

        let score: i32 = comments::table
            .find(&comment.id)
            .select(comments::score)
            .first(conn)?;
        Ok(VoteResponse {
            comment_id: comment.id,
            score,
            viewer_vote,
        })
    })
    .await?;
    Ok(Json(response))
}
