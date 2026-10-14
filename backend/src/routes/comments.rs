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
use crate::db::schema::{comment_votes, comments, versions};
use crate::domain::enums::CommentCategory;
use crate::domain::filter::Surface;
use crate::domain::moderation;
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::limit::Action;
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
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::CommentCategory>)]
    pub category: Option<String>,
    /// `open` or `resolved`. Omitted, both.
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::CommentState>)]
    pub state: Option<String>,
    /// Only this reviewer's notes.
    #[serde(default)]
    pub author_id: Option<String>,
    /// Only notes the room agreed with.
    #[serde(default)]
    pub min_score: Option<i32>,
}

/// Filters apply to the note, and a note's replies come with it.
///
/// Hiding a reply because it is uncategorised would leave an answer without
/// its question; the author is filtering *threads*, not sentences.
fn keeps(comment: &Comment, query: &ListQuery) -> bool {
    query
        .category
        .as_ref()
        .is_none_or(|c| &comment.category == c)
        // A removed note has no state worth filtering on; what is under it
        // does, and is filtered as part of its thread.
        && query
            .state
            .as_ref()
            .is_none_or(|s| &comment.state == s || comment.state == "deleted")
        && query
            .author_id
            .as_ref()
            .is_none_or(|a| &comment.author_id == a)
        && query.min_score.is_none_or(|m| comment.score >= m)
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
    /// How useful this reviewer's notes have proved elsewhere — votes plus the
    /// notes authors marked resolved. Shown beside the name so a reader with a
    /// record of being right is legible as one.
    pub author_score: i64,
    pub viewer_vote: i32,
    /// The label of the version it was written on: "v3".
    pub version_label: String,
    /// Written on an earlier version than readers are shown now. Its anchor
    /// is placed by its quote, where that text is still to be found.
    pub earlier_version: bool,
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
        let (publication, work) =
            support::publication_for_reader(conn, &publication_id, user.as_ref())?;

        // Deleted notes are loaded too, and dropped below unless someone has
        // answered them: a removed note with replies under it stays as an
        // empty placeholder, so the replies still have a question to answer.
        let mut q = comments::table
            .filter(comments::publication_id.eq(&publication_id))
            .into_boxed();
        if let Some(scene_id) = &query.scene_id {
            q = q.filter(comments::scene_id.eq(scene_id.clone()));
        }
        let mut rows: Vec<Comment> = q
            .order(comments::created_at.asc())
            .select(Comment::as_select())
            .load(conn)?;

        // Spoilers are withheld before anything else looks at the rows, so no
        // filter or sort downstream can leak one back.
        if let Some(reader) = user.as_ref() {
            if publication.spoiler_safe != 0 && !support::is_author(conn, &work, reader)? {
                let reached = support::scenes_reached(conn, &publication, reader)?;
                rows.retain(|c| match &c.scene_id {
                    Some(scene_id) => reached.contains(scene_id),
                    None => true,
                });
            }
        }

        // What moderation has done: removed notes stay as a placeholder (the
        // replies under them keep their question), held ones show only to
        // their author and to staff, and an account that is hidden is gone
        // from every public read.
        let hidden = crate::domain::restrictions::hidden_users(conn)?;
        let staff = moderation::is_staff(conn, viewer_id.as_deref())?;
        for c in rows.iter_mut() {
            match moderation::visibility(
                &c.moderation_state,
                &c.author_id,
                viewer_id.as_deref(),
                staff,
                &hidden,
            ) {
                None => c.state = "deleted".into(),
                Some(true) => {
                    c.body.clear();
                    c.anchor_quote = None;
                }
                Some(false) => {}
            }
        }

        // Replies survive their parent being filtered out only as far as
        // `build_threads` can find the parent, so filter roots and let the
        // thread come with them.
        let keep: std::collections::HashSet<String> = rows
            .iter()
            .filter(|c| c.parent_id.is_none() && keeps(c, &query))
            .map(|c| c.id.clone())
            .collect();
        rows.retain(|c| match &c.parent_id {
            Some(parent) => keep.contains(parent),
            None => keep.contains(&c.id),
        });
        rows = keep_answered(rows);

        let author_ids: Vec<String> = rows.iter().map(|c| c.author_id.clone()).collect();
        let authors = support::author_cards(conn, &author_ids)?;
        let standings = support::standings(conn, &author_ids)?;

        let votes: HashMap<String, i32> = match &viewer_id {
            Some(id) => comment_votes::table
                .filter(comment_votes::user_id.eq(id))
                .select((comment_votes::comment_id, comment_votes::value))
                .load::<(String, i32)>(conn)?
                .into_iter()
                .collect(),
            None => HashMap::new(),
        };

        let version_ids: Vec<String> = rows.iter().map(|c| c.version_id.clone()).collect();
        let labels: HashMap<String, String> = versions::table
            .filter(versions::id.eq_any(&version_ids))
            .select((versions::id, versions::label))
            .load::<(String, String)>(conn)?
            .into_iter()
            .collect();
        let mut views = build_threads(rows, &authors, &standings, &votes, sort_by_score);
        label_versions(&mut views, &labels, &publication.version_id);
        Ok(views)
    })
    .await?;
    Ok(Json(views))
}

/// Says which version each note was written on, and which were written on an
/// earlier one than readers are shown now.
fn label_versions(views: &mut [CommentView], labels: &HashMap<String, String>, current: &str) {
    for view in views {
        view.version_label = labels
            .get(&view.comment.version_id)
            .cloned()
            .unwrap_or_default();
        view.earlier_version = view.comment.version_id != current;
        label_versions(&mut view.replies, labels, current);
    }
}

/// Drops deleted notes nobody answered, and keeps the ones somebody did —
/// blank, as placeholders — along with every deleted note above a live reply.
fn keep_answered(rows: Vec<Comment>) -> Vec<Comment> {
    let parents: HashMap<String, Option<String>> = rows
        .iter()
        .map(|c| (c.id.clone(), c.parent_id.clone()))
        .collect();
    let mut answered: std::collections::HashSet<String> = Default::default();
    for live in rows.iter().filter(|c| c.state != "deleted") {
        let mut next = live.parent_id.clone();
        while let Some(id) = next {
            if !answered.insert(id.clone()) {
                break;
            }
            next = parents.get(&id).cloned().flatten();
        }
    }
    rows.into_iter()
        .filter(|c| c.state != "deleted" || answered.contains(&c.id))
        .map(|mut c| {
            if c.state == "deleted" {
                c.body.clear();
                c.anchor_quote = None;
            }
            c
        })
        .collect()
}

/// Flat rows in, nested threads out. Replies always read oldest-first; roots
/// respect the requested sort.
fn build_threads(
    rows: Vec<Comment>,
    authors: &HashMap<String, AuthorCard>,
    standings: &HashMap<String, support::Standing>,
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
        standings: &HashMap<String, support::Standing>,
        votes: &HashMap<String, i32>,
    ) -> CommentView {
        let replies = children
            .remove(&comment.id)
            .unwrap_or_default()
            .into_iter()
            .map(|child| to_view(child, children, authors, standings, votes))
            .collect();
        CommentView {
            author_score: standings
                .get(&comment.author_id)
                .map(|s| s.score)
                .unwrap_or_default(),
            author: authors
                .get(&comment.author_id)
                .cloned()
                .unwrap_or_else(|| AuthorCard {
                    id: comment.author_id.clone(),
                    display_name: "Unknown".into(),
                }),
            viewer_vote: votes.get(&comment.id).copied().unwrap_or(0),
            version_label: String::new(),
            earlier_version: false,
            replies,
            comment,
        }
    }

    roots
        .into_iter()
        .map(|root| to_view(root, &mut children, authors, standings, votes))
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
    state
        .limiter
        .check(&user.id, Action::Comment, state.config.comment_rate)?;
    // The word filter and the platform, before it is saved: a blocked comment
    // is refused here, a held or flagged one is acted on once it has an id.
    let screen = moderation::screen(&state, Surface::Comment, &user.id, &text).await?;

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
            body: screen.text.clone(),
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
        if let Some(original) = &screen.original {
            diesel::update(comments::table.find(&row.id))
                .set(comments::original_body.eq(Some(original.clone())))
                .execute(conn)?;
        }
        let provider = moderation::provider_name(conn)?;
        moderation::record(conn, &screen, "comment", &row.id, &user.id, &provider)?;

        let comment: Comment = comments::table
            .find(&row.id)
            .select(Comment::as_select())
            .first(conn)?;
        let authors = support::author_cards(conn, std::slice::from_ref(&comment.author_id))?;
        let standings = support::standings(conn, std::slice::from_ref(&comment.author_id))?;
        Ok(CommentView {
            author_score: standings
                .get(&comment.author_id)
                .map(|s| s.score)
                .unwrap_or_default(),
            author: authors
                .get(&comment.author_id)
                .cloned()
                .unwrap_or_else(|| AuthorCard {
                    id: comment.author_id.clone(),
                    display_name: "Unknown".into(),
                }),
            viewer_vote: 0,
            version_label: versions::table
                .find(&comment.version_id)
                .select(versions::label)
                .first(conn)?,
            earlier_version: false,
            replies: Vec::new(),
            comment,
        })
    })
    .await?;
    state.hub.comments_changed(&view.comment.publication_id);
    Ok(Json(view))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkStateRequest {
    /// `resolved` or `open`.
    #[schema(value_type = crate::domain::enums::CommentState)]
    pub state: String,
    /// Named ids win outright; without them the filters below choose.
    #[serde(default)]
    pub comment_ids: Vec<String>,
    #[serde(default)]
    pub scene_id: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::CommentCategory>)]
    pub category: Option<String>,
    #[serde(default)]
    pub author_id: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkStateResponse {
    pub changed: usize,
}

#[utoipa::path(
    post,
    path = "/api/publications/{publication_id}/comments/state",
    tag = "comments",
    params(("publication_id" = String, Path)),
    request_body = BulkStateRequest,
    responses(
        (status = 200, description = "How many moved", body = BulkStateResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
/// Resolve or reopen a pile of notes at once.
///
/// Twenty typo reports are twenty rows and one decision. Doing that one row at
/// a time is how a review queue stops being read at all.
pub async fn bulk_state(
    State(state): State<AppState>,
    user: AuthUser,
    Path(publication_id): Path<String>,
    Json(body): Json<BulkStateRequest>,
) -> ApiResult<Json<BulkStateResponse>> {
    if body.state != "open" && body.state != "resolved" {
        return Err(ApiError::BadRequest(
            "state must be `open` or `resolved`".into(),
        ));
    }
    // A bulk call with nothing to narrow it would resolve the whole review on
    // one mis-click, so it has to say what it means.
    if body.comment_ids.is_empty()
        && body.scene_id.is_none()
        && body.category.is_none()
        && body.author_id.is_none()
    {
        return Err(ApiError::BadRequest(
            "say which notes: commentIds, or a sceneId, category or authorId".into(),
        ));
    }

    let notify = publication_id.clone();
    let changed = blocking(&state.pool, move |conn| {
        support::publication_for_owner(conn, &publication_id, &user)?;
        let mut q = diesel::update(comments::table)
            .filter(comments::publication_id.eq(&publication_id))
            .filter(comments::state.ne("deleted"))
            .into_boxed();
        if !body.comment_ids.is_empty() {
            q = q.filter(comments::id.eq_any(body.comment_ids.clone()));
        }
        if let Some(scene_id) = &body.scene_id {
            q = q.filter(comments::scene_id.eq(scene_id.clone()));
        }
        if let Some(category) = &body.category {
            q = q.filter(comments::category.eq(category.clone()));
        }
        if let Some(author_id) = &body.author_id {
            q = q.filter(comments::author_id.eq(author_id.clone()));
        }
        Ok(q.set((
            comments::state.eq(&body.state),
            comments::updated_at.eq(now_iso()),
        ))
        .execute(conn)?)
    })
    .await?;
    if changed > 0 {
        state.hub.comments_changed(&notify);
    }
    Ok(Json(BulkStateResponse { changed }))
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
    // Editing the words is taking part in a discussion; resolving a note is
    // the work's author at their own desk, and a restriction on discussion
    // leaves that alone.
    let screen = match body
        .body
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        Some(text) => Some(moderation::screen(&state, Surface::Comment, &user.id, text).await?),
        None => None,
    };
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
            crate::domain::restrictions::check(
                conn,
                &user.id,
                crate::domain::restrictions::Capability::Discuss,
            )?;
            let screen = screen.clone().unwrap_or_default();
            diesel::update(comments::table.find(&comment.id))
                .set((
                    comments::body.eq(screen.text.trim()),
                    comments::original_body.eq(screen.original.clone()),
                ))
                .execute(conn)?;
            let provider = moderation::provider_name(conn)?;
            moderation::record(conn, &screen, "comment", &comment.id, &user.id, &provider)?;
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
    state.hub.comments_changed(&comment.publication_id);
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
    let publication = blocking(&state.pool, move |conn| {
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
        Ok(comment.publication_id)
    })
    .await?;
    state.hub.comments_changed(&publication);
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
    state
        .limiter
        .check(&user.id, Action::Vote, state.config.vote_rate)?;

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
        Ok((
            VoteResponse {
                comment_id: comment.id,
                score,
                viewer_vote,
            },
            comment.publication_id,
        ))
    })
    .await?;
    let (response, publication) = response;
    // Scores move the order of the thread, so it is a change readers see.
    state.hub.comments_changed(&publication);
    Ok(Json(response))
}

#[cfg(test)]
mod thread_tests {
    use super::*;

    fn note(id: &str, parent: Option<&str>, state: &str) -> Comment {
        Comment {
            id: id.into(),
            publication_id: "p".into(),
            version_id: "v".into(),
            scene_id: None,
            parent_id: parent.map(str::to_string),
            author_id: "a".into(),
            body: format!("words of {id}"),
            anchor: None,
            anchor_quote: Some("quote".into()),
            category: "general".into(),
            state: state.into(),
            score: 0,
            created_at: "t".into(),
            updated_at: "t".into(),
            moderation_state: "visible".into(),
            original_body: None,
        }
    }

    #[test]
    fn a_removed_note_stays_while_someone_has_answered_it() {
        let kept = keep_answered(vec![
            note("asked", None, "deleted"),
            note("answer", Some("asked"), "open"),
            note("forgotten", None, "deleted"),
            note("deep", None, "deleted"),
            note("middle", Some("deep"), "deleted"),
            note("leaf", Some("middle"), "open"),
        ]);
        let ids: Vec<&str> = kept.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["asked", "answer", "deep", "middle", "leaf"]);
        // Kept as a placeholder, not as what it said.
        assert!(kept[0].body.is_empty() && kept[0].anchor_quote.is_none());
        assert_eq!(kept[1].body, "words of answer");
    }
}
