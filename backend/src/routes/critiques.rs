//! Critiques of Daily Prompts answers: comments, and replies to comments, to
//! any depth. Names are always shown; there is no anonymous critique.
//!
//! Who sees them:
//! - On a submitted answer, every enrolled writer sees every critique, and so
//!   does its writer, enrolled or not.
//! - On a withdrawn answer, its writer sees them all, each critic sees their
//!   own, and nobody else sees any.
//!
//! A report goes to the answer's writer, who can remove the critique — the
//! same rule as reports on book comments. There is no site moderator yet.
use std::collections::{HashMap, HashSet};

use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::AuthUser;
use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::enums::{EntryState, ReportReason};
use crate::domain::new_id;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::limit::Action;
use crate::state::AppState;

use super::prompts::require_enrolled;
use super::support::{self, AuthorCard};

const MAX_BODY: usize = 10_000;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CritiqueView {
    pub id: String,
    pub entry_id: String,
    pub parent_id: Option<String>,
    /// Unset on a removed critique kept for its replies.
    pub author: Option<AuthorCard>,
    /// Empty when removed.
    pub body: String,
    pub removed: bool,
    pub edited_at: Option<String>,
    pub created_at: String,
    pub mine: bool,
    /// The caller may delete it: their own, or any on their own answer.
    pub can_remove: bool,
}

struct Access {
    entry: PromptEntry,
    /// The caller wrote the answer.
    author: bool,
}

fn entry_of(conn: &mut Conn, entry_id: &str) -> ApiResult<PromptEntry> {
    prompt_entries::table
        .find(entry_id)
        .select(PromptEntry::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("that answer is not here".into()))
}

/// Whether the caller may see any of the critiques on an entry, and how many.
fn access(conn: &mut Conn, entry_id: &str, user_id: &str) -> ApiResult<Access> {
    let entry = entry_of(conn, entry_id)?;
    if entry.author_id == user_id {
        return Ok(Access {
            entry,
            author: true,
        });
    }
    require_enrolled(conn, user_id)?;
    if entry.state == EntryState::Draft.as_str() {
        return Err(ApiError::NotFound("that answer is not here".into()));
    }
    Ok(Access {
        entry,
        author: false,
    })
}

/// Removed critiques stay only where a live reply sits somewhere beneath
/// them, so a thread keeps its shape and nothing else lingers.
fn keep_answered(rows: Vec<PromptCritique>) -> Vec<PromptCritique> {
    let parents: HashMap<String, Option<String>> = rows
        .iter()
        .map(|c| (c.id.clone(), c.parent_id.clone()))
        .collect();
    let mut answered: HashSet<String> = HashSet::new();
    for live in rows.iter().filter(|c| c.deleted_at.is_none()) {
        let mut next = live.parent_id.clone();
        while let Some(id) = next {
            if !answered.insert(id.clone()) {
                break;
            }
            next = parents.get(&id).cloned().flatten();
        }
    }
    rows.into_iter()
        .filter(|c| c.deleted_at.is_none() || answered.contains(&c.id))
        .collect()
}

fn view_all(
    conn: &mut Conn,
    rows: Vec<PromptCritique>,
    viewer: &str,
    entry_author: bool,
) -> ApiResult<Vec<CritiqueView>> {
    let ids: Vec<String> = rows.iter().map(|c| c.author_id.clone()).collect();
    let cards = support::author_cards(conn, &ids)?;
    Ok(rows
        .into_iter()
        .map(|c| {
            let removed = c.deleted_at.is_some();
            CritiqueView {
                author: (!removed)
                    .then(|| cards.get(&c.author_id).cloned())
                    .flatten(),
                body: if removed { String::new() } else { c.body },
                removed,
                mine: c.author_id == viewer,
                can_remove: !removed && (c.author_id == viewer || entry_author),
                edited_at: c.edited_at,
                created_at: c.created_at,
                parent_id: c.parent_id,
                entry_id: c.entry_id,
                id: c.id,
            }
        })
        .collect())
}

#[utoipa::path(
    get,
    path = "/api/prompt-entries/{entry_id}/critiques",
    tag = "critiques",
    params(("entry_id" = String, Path)),
    responses(
        (status = 200, description = "The whole tree, flat, oldest first; nest by parentId", body = Vec<CritiqueView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(entry_id): Path<String>,
) -> ApiResult<Json<Vec<CritiqueView>>> {
    let views = blocking(&state.pool, move |conn| {
        let access = access(conn, &entry_id, &user.id)?;
        let mut rows: Vec<PromptCritique> = prompt_critiques::table
            .filter(prompt_critiques::entry_id.eq(&entry_id))
            // Two in the same millisecond keep the order they were written in.
            .order((
                prompt_critiques::created_at.asc(),
                diesel::dsl::sql::<diesel::sql_types::BigInt>("prompt_critiques.rowid").asc(),
            ))
            .select(PromptCritique::as_select())
            .load(conn)?;
        // Withdrawn: the writer sees everything; a critic, only their own.
        if access.entry.state != EntryState::Submitted.as_str() && !access.author {
            rows.retain(|c| c.author_id == user.id && c.deleted_at.is_none());
        }
        view_all(conn, keep_answered(rows), &user.id, access.author)
    })
    .await?;
    Ok(Json(views))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CritiqueRequest {
    pub body: String,
    /// The critique this replies to.
    #[serde(default)]
    pub parent_id: Option<String>,
}

fn clean_body(body: &str) -> ApiResult<String> {
    let body = body.trim();
    if body.is_empty() {
        return Err(ApiError::BadRequest(
            "a critique needs something in it".into(),
        ));
    }
    if body.chars().count() > MAX_BODY {
        return Err(ApiError::BadRequest(format!(
            "a critique is at most {MAX_BODY} characters"
        )));
    }
    Ok(body.to_string())
}

/// What an inbox item about this entry says and links to.
fn item_detail(
    conn: &mut Conn,
    entry: &PromptEntry,
    critique_id: &str,
    role: Option<&str>,
) -> ApiResult<String> {
    let (day, body): (Option<String>, String) = daily_prompts::table
        .find(&entry.prompt_id)
        .select((daily_prompts::day, daily_prompts::body))
        .first(conn)?;
    let mut detail = json!({
        "day": day,
        "entryId": entry.id,
        "critiqueId": critique_id,
        "promptExcerpt": crate::domain::split::shorten(&body, 60),
    });
    if let Some(role) = role {
        detail["role"] = role.into();
    }
    Ok(detail.to_string())
}

#[utoipa::path(
    post,
    path = "/api/prompt-entries/{entry_id}/critiques",
    tag = "critiques",
    params(("entry_id" = String, Path)),
    request_body = CritiqueRequest,
    responses(
        (status = 200, description = "Posted", body = CritiqueView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(entry_id): Path<String>,
    Json(body): Json<CritiqueRequest>,
) -> ApiResult<Json<CritiqueView>> {
    let text = clean_body(&body.body)?;
    state
        .limiter
        .check(&user.id, Action::Critique, state.config.comment_rate)?;
    let view = blocking(&state.pool, move |conn| {
        require_enrolled(conn, &user.id)?;
        let entry = entry_of(conn, &entry_id)?;
        if entry.state != EntryState::Submitted.as_str() {
            return Err(ApiError::NotFound(
                "that answer is not open for critiques".into(),
            ));
        }
        let parent: Option<PromptCritique> = match &body.parent_id {
            Some(id) => Some(
                prompt_critiques::table
                    .find(id)
                    .filter(prompt_critiques::entry_id.eq(&entry.id))
                    .filter(prompt_critiques::deleted_at.is_null())
                    .select(PromptCritique::as_select())
                    .first(conn)
                    .optional()?
                    .ok_or_else(|| {
                        ApiError::NotFound("that critique is not here to reply to".into())
                    })?,
            ),
            None => None,
        };
        let now = clock::now_iso();
        let id = new_id();
        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::insert_into(prompt_critiques::table)
                .values(&NewPromptCritique {
                    id: id.clone(),
                    entry_id: entry.id.clone(),
                    parent_id: parent.as_ref().map(|p| p.id.clone()),
                    author_id: user.id.clone(),
                    body: text,
                    edited_at: None,
                    deleted_at: None,
                    created_at: now.clone(),
                    updated_at: now,
                })
                .execute(conn)?;
            // Who hears about it, each once, and never the person who wrote it.
            let mut told: HashSet<String> = HashSet::new();
            told.insert(user.id.clone());
            match &parent {
                None => {
                    if told.insert(entry.author_id.clone()) {
                        let detail = item_detail(conn, &entry, &id, None)?;
                        super::inbox::notify(
                            conn,
                            &entry.author_id,
                            "prompt.critique",
                            ("prompt_critique", &id),
                            Some(("prompt_entry", &entry.id)),
                            Some(&user.id),
                            &detail,
                        )?;
                    }
                }
                Some(p) => {
                    if told.insert(p.author_id.clone()) {
                        let detail = item_detail(conn, &entry, &id, Some("parent"))?;
                        super::inbox::notify(
                            conn,
                            &p.author_id,
                            "prompt.reply",
                            ("prompt_critique", &id),
                            Some(("prompt_entry", &entry.id)),
                            Some(&user.id),
                            &detail,
                        )?;
                    }
                    if told.insert(entry.author_id.clone()) {
                        let detail = item_detail(conn, &entry, &id, Some("entry"))?;
                        super::inbox::notify(
                            conn,
                            &entry.author_id,
                            "prompt.reply",
                            ("prompt_critique", &id),
                            Some(("prompt_entry", &entry.id)),
                            Some(&user.id),
                            &detail,
                        )?;
                    }
                }
            }
            Ok(())
        })?;
        let row: PromptCritique = prompt_critiques::table
            .find(&id)
            .select(PromptCritique::as_select())
            .first(conn)?;
        let author = entry.author_id == user.id;
        Ok(view_all(conn, vec![row], &user.id, author)?.remove(0))
    })
    .await?;
    Ok(Json(view))
}

fn critique_of(conn: &mut Conn, id: &str) -> ApiResult<PromptCritique> {
    prompt_critiques::table
        .find(id)
        .filter(prompt_critiques::deleted_at.is_null())
        .select(PromptCritique::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("that critique is not here".into()))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditCritiqueRequest {
    pub body: String,
}

#[utoipa::path(
    patch,
    path = "/api/prompt-critiques/{critique_id}",
    tag = "critiques",
    params(("critique_id" = String, Path)),
    request_body = EditCritiqueRequest,
    responses(
        (status = 200, description = "Saved, and marked edited", body = CritiqueView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(critique_id): Path<String>,
    Json(body): Json<EditCritiqueRequest>,
) -> ApiResult<Json<CritiqueView>> {
    let text = clean_body(&body.body)?;
    let view = blocking(&state.pool, move |conn| {
        let critique = critique_of(conn, &critique_id)?;
        if critique.author_id != user.id {
            return Err(ApiError::Forbidden(
                "only its writer can change a critique".into(),
            ));
        }
        // A critic keeps their own critique on a withdrawn answer, and may
        // still change it.
        let now = clock::now_iso();
        diesel::update(prompt_critiques::table.find(&critique.id))
            .set((
                prompt_critiques::body.eq(text),
                prompt_critiques::edited_at.eq(Some(&now)),
                prompt_critiques::updated_at.eq(&now),
            ))
            .execute(conn)?;
        let entry = entry_of(conn, &critique.entry_id)?;
        let row = critique_of(conn, &critique.id)?;
        Ok(view_all(conn, vec![row], &user.id, entry.author_id == user.id)?.remove(0))
    })
    .await?;
    Ok(Json(view))
}

/// Removes a critique: kept as a placeholder when it has replies, gone
/// otherwise, along with its inbox items and any placeholder above it left
/// with nothing beneath.
pub fn remove(conn: &mut Conn, critique: &PromptCritique) -> ApiResult<()> {
    conn.transaction::<_, ApiError, _>(|conn| {
        let replies: i64 = prompt_critiques::table
            .filter(prompt_critiques::parent_id.eq(&critique.id))
            .count()
            .get_result(conn)?;
        let now = clock::now_iso();
        if replies > 0 {
            diesel::update(prompt_critiques::table.find(&critique.id))
                .set((
                    prompt_critiques::body.eq(""),
                    prompt_critiques::deleted_at.eq(Some(&now)),
                    prompt_critiques::updated_at.eq(&now),
                ))
                .execute(conn)?;
            return Ok(());
        }
        let mut next = Some(critique.clone());
        while let Some(c) = next.take() {
            super::inbox::forget_subject(conn, "prompt_critique", &c.id)?;
            diesel::delete(prompt_critiques::table.find(&c.id)).execute(conn)?;
            let Some(parent_id) = &c.parent_id else { break };
            let parent: Option<PromptCritique> = prompt_critiques::table
                .find(parent_id)
                .select(PromptCritique::as_select())
                .first(conn)
                .optional()?;
            if let Some(p) = parent.filter(|p| p.deleted_at.is_some()) {
                let left: i64 = prompt_critiques::table
                    .filter(prompt_critiques::parent_id.eq(&p.id))
                    .count()
                    .get_result(conn)?;
                if left == 0 {
                    next = Some(p);
                }
            }
        }
        Ok(())
    })
}

#[utoipa::path(
    delete,
    path = "/api/prompt-critiques/{critique_id}",
    tag = "critiques",
    params(("critique_id" = String, Path)),
    responses(
        (status = 200, description = "Removed", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_critique(
    State(state): State<AppState>,
    user: AuthUser,
    Path(critique_id): Path<String>,
) -> ApiResult<Json<Value>> {
    blocking(&state.pool, move |conn| {
        let critique = critique_of(conn, &critique_id)?;
        let entry = entry_of(conn, &critique.entry_id)?;
        if critique.author_id != user.id && entry.author_id != user.id {
            return Err(ApiError::Forbidden(
                "a critique can be removed by its writer, or by the writer of the answer".into(),
            ));
        }
        remove(conn, &critique)
    })
    .await?;
    Ok(Json(json!({ "removed": true })))
}

#[utoipa::path(
    post,
    path = "/api/prompt-critiques/{critique_id}/report",
    tag = "critiques",
    params(("critique_id" = String, Path)),
    request_body = super::moderation::ReportRequest,
    responses(
        (status = 200, description = "Reported to the answer's writer", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn report(
    State(state): State<AppState>,
    user: AuthUser,
    Path(critique_id): Path<String>,
    Json(body): Json<super::moderation::ReportRequest>,
) -> ApiResult<Json<Value>> {
    if !ReportReason::accepts(&body.reason) {
        return Err(ApiError::BadRequest(format!(
            "reason must be one of {}",
            ReportReason::options()
        )));
    }
    blocking(&state.pool, move |conn| {
        let critique = critique_of(conn, &critique_id)?;
        let access = access(conn, &critique.entry_id, &user.id)?;
        if access.entry.state != EntryState::Submitted.as_str() && !access.author {
            return Err(ApiError::NotFound("that critique is not here".into()));
        }
        if critique.author_id == user.id {
            return Err(ApiError::BadRequest(
                "reporting your own critique does nothing — delete it instead".into(),
            ));
        }
        if access.author {
            return Err(ApiError::BadRequest(
                "it is on your own answer: you can remove it yourself".into(),
            ));
        }
        let existing: Option<String> = prompt_critique_reports::table
            .filter(prompt_critique_reports::critique_id.eq(&critique.id))
            .filter(prompt_critique_reports::reporter_id.eq(&user.id))
            .select(prompt_critique_reports::id)
            .first(conn)
            .optional()?;
        conn.transaction::<_, ApiError, _>(|conn| {
            match existing {
                // The same report again is the same report, not a louder one.
                Some(id) => {
                    diesel::update(prompt_critique_reports::table.find(&id))
                        .set((
                            prompt_critique_reports::reason.eq(&body.reason),
                            prompt_critique_reports::detail.eq(body.detail.clone()),
                        ))
                        .execute(conn)?;
                }
                None => {
                    let report_id = new_id();
                    diesel::insert_into(prompt_critique_reports::table)
                        .values(&NewPromptCritiqueReport {
                            id: report_id.clone(),
                            critique_id: critique.id.clone(),
                            reporter_id: user.id.clone(),
                            reason: body.reason.clone(),
                            detail: body.detail.clone(),
                            created_at: clock::now_iso(),
                        })
                        .execute(conn)?;
                    let detail = item_detail(conn, &access.entry, &critique.id, None)?;
                    super::inbox::notify(
                        conn,
                        &access.entry.author_id,
                        "prompt.report",
                        ("prompt_critique", &critique.id),
                        Some(("prompt_entry", &access.entry.id)),
                        Some(&user.id),
                        &detail,
                    )?;
                }
            }
            Ok(())
        })
    })
    .await?;
    Ok(Json(json!({ "reported": true })))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CritiqueReportView {
    #[serde(flatten)]
    pub report: PromptCritiqueReport,
    pub reporter: AuthorCard,
    pub critique: CritiqueView,
}

#[utoipa::path(
    get,
    path = "/api/prompt-entries/{entry_id}/reports",
    tag = "critiques",
    params(("entry_id" = String, Path)),
    responses(
        (status = 200, description = "What was reported on the caller's own answer", body = Vec<CritiqueReportView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_reports(
    State(state): State<AppState>,
    user: AuthUser,
    Path(entry_id): Path<String>,
) -> ApiResult<Json<Vec<CritiqueReportView>>> {
    let views = blocking(&state.pool, move |conn| {
        let entry = entry_of(conn, &entry_id)?;
        if entry.author_id != user.id {
            return Err(ApiError::Forbidden(
                "reports go to the answer's writer".into(),
            ));
        }
        let rows: Vec<(PromptCritiqueReport, PromptCritique)> = prompt_critique_reports::table
            .inner_join(prompt_critiques::table)
            .filter(prompt_critiques::entry_id.eq(&entry.id))
            .filter(prompt_critiques::deleted_at.is_null())
            .order(prompt_critique_reports::created_at.desc())
            .select((
                PromptCritiqueReport::as_select(),
                PromptCritique::as_select(),
            ))
            .load(conn)?;
        let reporters: Vec<String> = rows.iter().map(|(r, _)| r.reporter_id.clone()).collect();
        let cards = support::author_cards(conn, &reporters)?;
        let mut out = Vec::new();
        for (report, critique) in rows {
            let critique = view_all(conn, vec![critique], &user.id, true)?.remove(0);
            out.push(CritiqueReportView {
                reporter: cards
                    .get(&report.reporter_id)
                    .cloned()
                    .unwrap_or(AuthorCard {
                        id: report.reporter_id.clone(),
                        display_name: "Unknown".into(),
                    }),
                critique,
                report,
            });
        }
        Ok(out)
    })
    .await?;
    Ok(Json(views))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, parent: Option<&str>, deleted: bool) -> PromptCritique {
        PromptCritique {
            id: id.into(),
            entry_id: "e".into(),
            parent_id: parent.map(str::to_string),
            author_id: "a".into(),
            body: if deleted {
                String::new()
            } else {
                "words".into()
            },
            edited_at: None,
            deleted_at: deleted.then(|| "t".to_string()),
            created_at: id.into(),
            updated_at: id.into(),
        }
    }

    #[test]
    fn a_removed_critique_stays_only_above_a_live_reply() {
        let rows = vec![
            c("1", None, true),
            c("2", Some("1"), true),
            c("3", Some("2"), false),
            c("4", None, true),
            c("5", Some("4"), true),
        ];
        let kept: Vec<String> = keep_answered(rows).into_iter().map(|c| c.id).collect();
        assert_eq!(kept, ["1", "2", "3"]);
    }

    #[test]
    fn a_thread_twenty_deep_keeps_every_level() {
        let mut rows = vec![c("00", None, false)];
        for i in 1..20 {
            rows.push(c(&format!("{i:02}"), Some(&format!("{:02}", i - 1)), false));
        }
        assert_eq!(keep_answered(rows).len(), 20);
    }
}
