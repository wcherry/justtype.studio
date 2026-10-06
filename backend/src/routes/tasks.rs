//! The author's list of what to change.
//!
//! A review ends with forty notes and no plan. A revision task is the step
//! between: the author decides a note is real, and it becomes a line of work
//! attached to the scene it belongs to — outliving the publication it came
//! from, because the next draft is where it gets answered.
//!
//! This is deliberately not `tracked_changes`, which proposes an exact edit to
//! an exact range. "Chapter four needs to be colder" is not a diff.
use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{comments, revision_tasks};
use crate::domain::enums::{ActivityKind, TaskState};
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::collaborators::record;
use super::support::{self, Ability};

/// Enough of a passage to recognise it; the scene holds the rest.
const MAX_QUOTE: usize = 1000;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskQuery {
    /// `open` (the default), `done`, or `all`.
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub scene_id: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateTaskRequest {
    pub body: String,
    #[serde(default)]
    pub scene_id: Option<String>,
    /// The passage it is about, when it was written against one.
    #[serde(default)]
    pub quote: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTaskRequest {
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::TaskState>)]
    pub state: Option<String>,
    #[serde(default)]
    pub scene_id: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskView {
    #[serde(flatten)]
    pub task: RevisionTask,
    /// The note it came from, if it came from one and that note still stands.
    pub source: Option<TaskSource>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskSource {
    pub comment_id: String,
    pub publication_id: String,
    pub author: support::AuthorCard,
    pub quote: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/works/{work_id}/tasks",
    tag = "tasks",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "What is left to do", body = Vec<TaskView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Query(query): Query<TaskQuery>,
) -> ApiResult<Json<Vec<TaskView>>> {
    let views = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Read)?;
        let mut q = revision_tasks::table
            .filter(revision_tasks::work_id.eq(&work_id))
            .into_boxed();
        match query.state.as_deref().unwrap_or("open") {
            "all" => {}
            wanted => {
                if !TaskState::accepts(wanted) {
                    return Err(ApiError::BadRequest(format!(
                        "state must be one of {}, or `all`",
                        TaskState::options()
                    )));
                }
                q = q.filter(revision_tasks::state.eq(wanted.to_string()));
            }
        }
        if let Some(scene_id) = &query.scene_id {
            q = q.filter(revision_tasks::scene_id.eq(scene_id.clone()));
        }
        let rows: Vec<RevisionTask> = q
            .order(revision_tasks::created_at.asc())
            .select(RevisionTask::as_select())
            .load(conn)?;
        with_sources(conn, rows)
    })
    .await?;
    Ok(Json(views))
}

/// Attaches the note each task came from, in two queries rather than two per
/// task.
fn with_sources(conn: &mut crate::db::Conn, rows: Vec<RevisionTask>) -> ApiResult<Vec<TaskView>> {
    let comment_ids: Vec<String> = rows.iter().filter_map(|t| t.comment_id.clone()).collect();
    let sources: Vec<Comment> = if comment_ids.is_empty() {
        Vec::new()
    } else {
        comments::table
            .filter(comments::id.eq_any(&comment_ids))
            .filter(comments::state.ne("deleted"))
            .select(Comment::as_select())
            .load(conn)?
    };
    let author_ids: Vec<String> = sources.iter().map(|c| c.author_id.clone()).collect();
    let cards = support::author_cards(conn, &author_ids)?;

    Ok(rows
        .into_iter()
        .map(|task| {
            let source = task.comment_id.as_ref().and_then(|id| {
                let comment = sources.iter().find(|c| &c.id == id)?;
                Some(TaskSource {
                    comment_id: comment.id.clone(),
                    publication_id: comment.publication_id.clone(),
                    author: cards
                        .get(&comment.author_id)
                        .cloned()
                        .unwrap_or(support::AuthorCard {
                            id: comment.author_id.clone(),
                            display_name: "Unknown".into(),
                        }),
                    quote: comment.anchor_quote.clone(),
                })
            });
            TaskView { task, source }
        })
        .collect())
}

#[utoipa::path(
    post,
    path = "/api/works/{work_id}/tasks",
    tag = "tasks",
    params(("work_id" = String, Path)),
    request_body = CreateTaskRequest,
    responses(
        (status = 200, description = "Added", body = TaskView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<CreateTaskRequest>,
) -> ApiResult<Json<TaskView>> {
    if body.body.trim().is_empty() {
        return Err(ApiError::BadRequest("a task needs a line of text".into()));
    }
    let quote = body
        .quote
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(|q| q.chars().take(MAX_QUOTE).collect::<String>());
    if quote.is_some() && body.scene_id.is_none() {
        return Err(ApiError::BadRequest(
            "a quoted passage needs the scene it is in".into(),
        ));
    }
    let view = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Suggest)?;
        if let Some(scene_id) = &body.scene_id {
            support::scene_for(conn, scene_id, &user, Ability::Suggest)?;
        }
        let row = insert(
            conn,
            &work_id,
            &user.id,
            body.scene_id.clone(),
            None,
            body.body.trim(),
            quote,
        )?;
        Ok(with_sources(conn, vec![row])?.remove(0))
    })
    .await?;
    Ok(Json(view))
}

fn insert(
    conn: &mut crate::db::Conn,
    work_id: &str,
    actor_id: &str,
    scene_id: Option<String>,
    comment_id: Option<String>,
    body: &str,
    quote: Option<String>,
) -> ApiResult<RevisionTask> {
    let now = now_iso();
    let row = NewRevisionTask {
        id: new_id(),
        work_id: work_id.to_string(),
        scene_id,
        comment_id,
        body: body.to_string(),
        state: "open".into(),
        created_at: now.clone(),
        updated_at: now,
        done_at: None,
        quote,
    };
    diesel::insert_into(revision_tasks::table)
        .values(&row)
        .execute(conn)?;
    record(
        conn,
        work_id,
        actor_id,
        ActivityKind::TaskAdded,
        Some(&row.id),
        &headline(body),
    )?;
    Ok(revision_tasks::table
        .find(&row.id)
        .select(RevisionTask::as_select())
        .first(conn)?)
}

/// A task as one line of the activity feed: a reviewer's note can run to
/// paragraphs, and the feed only needs to say which one.
fn headline(body: &str) -> String {
    const LONGEST: usize = 120;
    let first = body.lines().next().unwrap_or_default().trim();
    if first.chars().count() <= LONGEST {
        first.to_string()
    } else {
        let cut: String = first.chars().take(LONGEST).collect();
        format!("{}…", cut.trim_end())
    }
}

#[utoipa::path(
    post,
    path = "/api/comments/{comment_id}/task",
    tag = "tasks",
    params(("comment_id" = String, Path)),
    responses(
        (status = 200, description = "On the list", body = TaskView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
/// Turn a reviewer's note into a line of the author's own work.
pub async fn from_comment(
    State(state): State<AppState>,
    user: AuthUser,
    Path(comment_id): Path<String>,
) -> ApiResult<Json<TaskView>> {
    let view = blocking(&state.pool, move |conn| {
        let comment: Comment = comments::table
            .find(&comment_id)
            .filter(comments::state.ne("deleted"))
            .select(Comment::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("comment not found".into()))?;
        let (_, work) = support::publication_for_owner(conn, &comment.publication_id, &user)?;

        // Already on the list: hand back the one that is there rather than a
        // second copy of the same decision.
        let existing: Option<RevisionTask> = revision_tasks::table
            .filter(revision_tasks::comment_id.eq(&comment.id))
            .select(RevisionTask::as_select())
            .first(conn)
            .optional()?;
        let row = match existing {
            Some(row) => row,
            None => insert(
                conn,
                &work.id,
                &user.id,
                comment.scene_id.clone(),
                Some(comment.id.clone()),
                &comment.body,
                // The note's own passage is shown through its source.
                None,
            )?,
        };
        Ok(with_sources(conn, vec![row])?.remove(0))
    })
    .await?;
    Ok(Json(view))
}

#[utoipa::path(
    patch,
    path = "/api/tasks/{task_id}",
    tag = "tasks",
    params(("task_id" = String, Path)),
    request_body = UpdateTaskRequest,
    responses(
        (status = 200, description = "Updated", body = TaskView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(task_id): Path<String>,
    Json(body): Json<UpdateTaskRequest>,
) -> ApiResult<Json<TaskView>> {
    let view = blocking(&state.pool, move |conn| {
        let task = task_for(conn, &task_id, &user, Ability::Suggest)?;
        let now = now_iso();

        if let Some(text) = body.body.as_ref().map(|b| b.trim()) {
            if text.is_empty() {
                return Err(ApiError::BadRequest("a task needs a line of text".into()));
            }
            diesel::update(revision_tasks::table.find(&task.id))
                .set(revision_tasks::body.eq(text))
                .execute(conn)?;
        }
        if let Some(new_state) = &body.state {
            if !TaskState::accepts(new_state) {
                return Err(ApiError::BadRequest(format!(
                    "state must be one of {}",
                    TaskState::options()
                )));
            }
            diesel::update(revision_tasks::table.find(&task.id))
                .set((
                    revision_tasks::state.eq(new_state),
                    // Kept so "what did I get through this week" is answerable.
                    revision_tasks::done_at.eq((new_state == "done").then(|| now.clone())),
                ))
                .execute(conn)?;
            if new_state == "done" && task.state != "done" {
                record(
                    conn,
                    &task.work_id,
                    &user.id,
                    ActivityKind::TaskDone,
                    Some(&task.id),
                    &headline(&task.body),
                )?;
            }
        }
        if let Some(scene_id) = &body.scene_id {
            support::scene_for(conn, scene_id, &user, Ability::Suggest)?;
            diesel::update(revision_tasks::table.find(&task.id))
                .set(revision_tasks::scene_id.eq(scene_id))
                .execute(conn)?;
        }
        diesel::update(revision_tasks::table.find(&task.id))
            .set(revision_tasks::updated_at.eq(&now))
            .execute(conn)?;

        let row: RevisionTask = revision_tasks::table
            .find(&task.id)
            .select(RevisionTask::as_select())
            .first(conn)?;
        Ok(with_sources(conn, vec![row])?.remove(0))
    })
    .await?;
    Ok(Json(view))
}

#[utoipa::path(
    delete,
    path = "/api/tasks/{task_id}",
    tag = "tasks",
    params(("task_id" = String, Path)),
    responses(
        (status = 200, description = "Gone", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_task(
    State(state): State<AppState>,
    user: AuthUser,
    Path(task_id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    blocking(&state.pool, move |conn| {
        let task = task_for(conn, &task_id, &user, Ability::Suggest)?;
        diesel::delete(revision_tasks::table.find(&task.id)).execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

fn task_for(
    conn: &mut crate::db::Conn,
    task_id: &str,
    user: &AuthUser,
    need: Ability,
) -> ApiResult<RevisionTask> {
    let task: RevisionTask = revision_tasks::table
        .find(task_id)
        .select(RevisionTask::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("no such task".into()))?;
    support::work_for(conn, &task.work_id, user, need)?;
    Ok(task)
}
