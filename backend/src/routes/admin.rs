//! For site administrators: the daily prompt queue, and how the scheduled
//! jobs are doing. Every route here takes an [`AdminUser`], so anyone else
//! gets a 403.
//!
//! Admins are made on the command line (`cargo xtask admin grant <email>`),
//! never through the API; see [`grant`].
use axum::extract::{Path, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::AdminUser;
use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::enums::AuditAction;
use crate::domain::new_id;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

/// Fewer than this many queued, and the admin page says so.
pub const LOW_QUEUE: usize = 7;
const MAX_PROMPT: usize = 2_000;

// --- the command line ----------------------------------------------------

/// Makes `email` an admin, or stops them being one, and records it. Called by
/// `--admin grant|revoke <email>`, which `cargo xtask admin` runs.
pub fn grant(conn: &mut Conn, email: &str, admin: bool) -> ApiResult<String> {
    let (id, name): (String, String) = users::table
        .filter(users::email.eq(email.trim().to_lowercase()))
        .select((users::id, users::display_name))
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound(format!("no account has the address {email}")))?;
    diesel::update(users::table.find(&id))
        .set(users::is_admin.eq(admin))
        .execute(conn)?;
    super::audit::record(
        conn,
        Some(&id),
        None,
        if admin {
            AuditAction::AdminGranted
        } else {
            AuditAction::AdminRevoked
        },
        &format!("{name} <{email}>, from the command line"),
    )?;
    Ok(format!(
        "{name} <{email}> {} a site administrator",
        if admin { "is now" } else { "is no longer" }
    ))
}

// --- the prompt queue ----------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminDay {
    pub day: String,
    pub prompt: Option<DailyPrompt>,
    /// Answers started, drafts included: a prompt with any cannot be deleted.
    pub entries: i64,
    pub opened: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminPrompts {
    pub today: String,
    /// The queue, in the order it will be used.
    pub queue: Vec<DailyPrompt>,
    /// Tomorrow and the two weeks after it, then today and the two weeks before.
    pub days: Vec<AdminDay>,
    /// Set when fewer than seven are queued.
    pub warning: Option<String>,
}

fn entries_for(conn: &mut Conn, prompt_id: &str) -> ApiResult<i64> {
    Ok(prompt_entries::table
        .filter(prompt_entries::prompt_id.eq(prompt_id))
        .count()
        .get_result(conn)?)
}

fn overview(conn: &mut Conn) -> ApiResult<AdminPrompts> {
    let today = super::prompts::today();
    let queue: Vec<DailyPrompt> = daily_prompts::table
        .filter(daily_prompts::day.is_null())
        .order((
            daily_prompts::position.asc(),
            daily_prompts::created_at.asc(),
        ))
        .select(DailyPrompt::as_select())
        .load(conn)?;
    let mut days = Vec::new();
    // Tomorrow first (it is the one being prepared), then the days after it,
    // then today and the past, most recent first.
    let order = (1..=14).chain(std::iter::once(0)).chain((-14..0).rev());
    for offset in order {
        let day = today + chrono::Duration::days(offset);
        let day_s = clock::day_string(day);
        let prompt: Option<DailyPrompt> = daily_prompts::table
            .filter(daily_prompts::day.eq(&day_s))
            .select(DailyPrompt::as_select())
            .first(conn)
            .optional()?;
        // Past days with nothing are not worth a row; future ones are, so a
        // pin has somewhere to go.
        if prompt.is_none() && offset < 0 {
            continue;
        }
        let entries = match &prompt {
            Some(p) => entries_for(conn, &p.id)?,
            None => 0,
        };
        days.push(AdminDay {
            day: day_s,
            prompt,
            entries,
            opened: offset <= 0,
        });
    }
    let warning = (queue.len() < LOW_QUEUE).then(|| {
        format!(
            "Only {} prompt{} queued. When the queue is empty, the model writes one each day.",
            queue.len(),
            if queue.len() == 1 { " is" } else { "s are" }
        )
    });
    Ok(AdminPrompts {
        today: clock::day_string(today),
        queue,
        days,
        warning,
    })
}

#[utoipa::path(
    get,
    path = "/api/admin/prompts",
    tag = "admin",
    responses(
        (status = 200, description = "The queue and the days around today", body = AdminPrompts),
        (status = 403, description = "Not an admin", body = ErrorEnvelope),
    )
)]
pub async fn list_prompts(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> ApiResult<Json<AdminPrompts>> {
    Ok(Json(blocking(&state.pool, overview).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewPromptRequest {
    pub body: String,
    /// Pin it to this future day instead of queueing it.
    #[serde(default)]
    pub day: Option<String>,
}

fn clean_prompt(body: &str) -> ApiResult<String> {
    let body = body.trim();
    if body.is_empty() {
        return Err(ApiError::BadRequest("a prompt needs some words".into()));
    }
    if body.chars().count() > MAX_PROMPT {
        return Err(ApiError::BadRequest(format!(
            "a prompt is at most {MAX_PROMPT} characters"
        )));
    }
    Ok(body.to_string())
}

/// A day a prompt can be pinned to: one that has not opened, with nothing
/// else pinned to it.
fn pinnable(conn: &mut Conn, day: &str, except: Option<&str>) -> ApiResult<()> {
    let parsed = clock::parse_day(day)
        .ok_or_else(|| ApiError::BadRequest("a day looks like YYYY-MM-DD".into()))?;
    if parsed <= super::prompts::today() {
        return Err(ApiError::BadRequest(
            "that day has already opened; pin to a future day".into(),
        ));
    }
    let taken: Option<String> = daily_prompts::table
        .filter(daily_prompts::day.eq(day))
        .select(daily_prompts::id)
        .first(conn)
        .optional()?;
    match taken {
        Some(id) if Some(id.as_str()) != except => Err(ApiError::Conflict(format!(
            "{day} already has a prompt: unpin it first, or edit that one"
        ))),
        _ => Ok(()),
    }
}

fn next_position(conn: &mut Conn) -> ApiResult<i32> {
    let last: Option<i32> = daily_prompts::table
        .filter(daily_prompts::day.is_null())
        .select(diesel::dsl::max(daily_prompts::position))
        .first(conn)?;
    Ok(last.map_or(0, |p| p + 1))
}

#[utoipa::path(
    post,
    path = "/api/admin/prompts",
    tag = "admin",
    request_body = NewPromptRequest,
    responses(
        (status = 200, description = "Queued, or pinned", body = AdminPrompts),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create_prompt(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Json(body): Json<NewPromptRequest>,
) -> ApiResult<Json<AdminPrompts>> {
    let text = clean_prompt(&body.body)?;
    let page = blocking(&state.pool, move |conn| {
        if let Some(day) = &body.day {
            pinnable(conn, day, None)?;
        }
        let now = clock::now_iso();
        let position = match body.day {
            Some(_) => None,
            None => Some(next_position(conn)?),
        };
        diesel::insert_into(daily_prompts::table)
            .values(&NewDailyPrompt {
                id: new_id(),
                day: body.day.clone(),
                position,
                body: text,
                source: "curated".into(),
                created_by: Some(admin.id.clone()),
                created_at: now.clone(),
                updated_at: now,
            })
            .execute(conn)?;
        overview(conn)
    })
    .await?;
    Ok(Json(page))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePromptRequest {
    #[serde(default)]
    pub body: Option<String>,
    /// Pin to this future day.
    #[serde(default)]
    pub day: Option<String>,
    /// Take it off its day and put it at the end of the queue.
    #[serde(default)]
    pub unpin: bool,
}

#[utoipa::path(
    patch,
    path = "/api/admin/prompts/{prompt_id}",
    tag = "admin",
    params(("prompt_id" = String, Path)),
    request_body = UpdatePromptRequest,
    responses(
        (status = 200, description = "Saved", body = AdminPrompts),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update_prompt(
    State(state): State<AppState>,
    AdminUser(admin): AdminUser,
    Path(prompt_id): Path<String>,
    Json(body): Json<UpdatePromptRequest>,
) -> ApiResult<Json<AdminPrompts>> {
    let text = body.body.as_deref().map(clean_prompt).transpose()?;
    let page = blocking(&state.pool, move |conn| {
        let prompt: DailyPrompt = daily_prompts::table
            .find(&prompt_id)
            .select(DailyPrompt::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("that prompt is not here".into()))?;
        let opened = prompt
            .day
            .as_deref()
            .and_then(clock::parse_day)
            .is_some_and(|d| d <= super::prompts::today());
        if opened && (body.day.is_some() || body.unpin) {
            return Err(ApiError::BadRequest(
                "its day has opened, so it stays on it".into(),
            ));
        }
        let now = clock::now_iso();
        conn.transaction::<_, ApiError, _>(|conn| {
            if let Some(text) = &text {
                if text != &prompt.body {
                    diesel::update(daily_prompts::table.find(&prompt.id))
                        .set((
                            daily_prompts::body.eq(text),
                            daily_prompts::updated_at.eq(&now),
                        ))
                        .execute(conn)?;
                    // Writers may already be answering it: a change after it
                    // opened is on the record.
                    if opened {
                        super::audit::record(
                            conn,
                            Some(&admin.id),
                            None,
                            AuditAction::PromptEdited,
                            &format!(
                                "the prompt for {} changed from “{}” to “{}”",
                                prompt.day.clone().unwrap_or_default(),
                                crate::domain::split::shorten(&prompt.body, 80),
                                crate::domain::split::shorten(text, 80),
                            ),
                        )?;
                    }
                }
            }
            if let Some(day) = &body.day {
                pinnable(conn, day, Some(&prompt.id))?;
                diesel::update(daily_prompts::table.find(&prompt.id))
                    .set((
                        daily_prompts::day.eq(Some(day)),
                        daily_prompts::position.eq(None::<i32>),
                        daily_prompts::updated_at.eq(&now),
                    ))
                    .execute(conn)?;
            } else if body.unpin && prompt.day.is_some() {
                let position = next_position(conn)?;
                diesel::update(daily_prompts::table.find(&prompt.id))
                    .set((
                        daily_prompts::day.eq(None::<String>),
                        daily_prompts::position.eq(Some(position)),
                        daily_prompts::updated_at.eq(&now),
                    ))
                    .execute(conn)?;
            }
            Ok(())
        })?;
        overview(conn)
    })
    .await?;
    Ok(Json(page))
}

#[utoipa::path(
    delete,
    path = "/api/admin/prompts/{prompt_id}",
    tag = "admin",
    params(("prompt_id" = String, Path)),
    responses(
        (status = 200, description = "Deleted", body = AdminPrompts),
        (status = 409, description = "Someone has written to it", body = ErrorEnvelope),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_prompt(
    State(state): State<AppState>,
    _admin: AdminUser,
    Path(prompt_id): Path<String>,
) -> ApiResult<Json<AdminPrompts>> {
    let page = blocking(&state.pool, move |conn| {
        if entries_for(conn, &prompt_id)? > 0 {
            return Err(ApiError::Conflict(
                "writers have answered this prompt, so it stays; edit it instead".into(),
            ));
        }
        let gone = diesel::delete(daily_prompts::table.find(&prompt_id)).execute(conn)?;
        if gone == 0 {
            return Err(ApiError::NotFound("that prompt is not here".into()));
        }
        overview(conn)
    })
    .await?;
    Ok(Json(page))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReorderPromptsRequest {
    /// Every queued prompt's id, in the new order.
    pub ids: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/prompts/reorder",
    tag = "admin",
    request_body = ReorderPromptsRequest,
    responses(
        (status = 200, description = "Reordered", body = AdminPrompts),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn reorder_prompts(
    State(state): State<AppState>,
    _admin: AdminUser,
    Json(body): Json<ReorderPromptsRequest>,
) -> ApiResult<Json<AdminPrompts>> {
    let page = blocking(&state.pool, move |conn| {
        let mut queued: Vec<String> = daily_prompts::table
            .filter(daily_prompts::day.is_null())
            .select(daily_prompts::id)
            .load(conn)?;
        let mut asked = body.ids.clone();
        queued.sort();
        asked.sort();
        if queued != asked {
            return Err(ApiError::BadRequest(
                "send every queued prompt's id, each once".into(),
            ));
        }
        conn.transaction::<_, ApiError, _>(|conn| {
            for (i, id) in body.ids.iter().enumerate() {
                diesel::update(daily_prompts::table.find(id))
                    .set(daily_prompts::position.eq(Some(i as i32)))
                    .execute(conn)?;
            }
            Ok(())
        })?;
        overview(conn)
    })
    .await?;
    Ok(Json(page))
}

// --- the jobs --------------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobsReport {
    pub health: crate::jobs::JobHealth,
    /// The most recent runs, newest first.
    pub runs: Vec<JobRun>,
    pub mail_waiting: i64,
    pub mail_failed: i64,
    /// Public posts waiting for the moderation platform to come back.
    pub external_backlog: i64,
    /// Events from the platform that were stored and not applied.
    pub external_unapplied: Vec<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/jobs",
    tag = "admin",
    responses(
        (status = 200, description = "What ran, what failed, what is late", body = JobsReport),
        (status = 403, description = "Not an admin", body = ErrorEnvelope),
    )
)]
pub async fn jobs(State(state): State<AppState>, _admin: AdminUser) -> ApiResult<Json<JobsReport>> {
    let started =
        clock::now() - chrono::Duration::from_std(state.started_at.elapsed()).unwrap_or_default();
    let report = blocking(&state.pool, move |conn| {
        let health = crate::jobs::health(conn, clock::now(), started)?;
        let runs: Vec<JobRun> = job_runs::table
            .order(job_runs::started_at.desc())
            .limit(50)
            .select(JobRun::as_select())
            .load(conn)?;
        let mail_waiting: i64 = mail_queue::table
            .filter(mail_queue::sent_at.is_null())
            .filter(mail_queue::failed_at.is_null())
            .count()
            .get_result(conn)?;
        let mail_failed: i64 = mail_queue::table
            .filter(mail_queue::failed_at.is_not_null())
            .count()
            .get_result(conn)?;
        let external_backlog: i64 = external_checks::table
            .filter(external_checks::state.eq("pending"))
            .count()
            .get_result(conn)?;
        let external_unapplied: Vec<String> = external_events::table
            .filter(external_events::error.is_not_null())
            .order(external_events::received_at.desc())
            .limit(20)
            .select((external_events::kind, external_events::error))
            .load::<(String, Option<String>)>(conn)?
            .into_iter()
            .map(|(kind, error)| format!("{kind}: {}", error.unwrap_or_default()))
            .collect();
        Ok(JobsReport {
            health,
            runs,
            mail_waiting,
            mail_failed,
            external_backlog,
            external_unapplied,
        })
    })
    .await?;
    Ok(Json(report))
}

// --- the test clock ----------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TestClockRequest {
    /// RFC 3339. Unset, the clock is not moved and the jobs just run.
    #[serde(default)]
    pub now: Option<String>,
}

/// Mounted only under `JUSTTYPE_TEST_CLOCK=1`, for the smoke walkthrough:
/// moves the clock and runs a tick at once, so a day can be advanced without
/// waiting for one. Not in the OpenAPI document: no client should know it.
pub async fn set_test_clock(
    State(state): State<AppState>,
    Json(body): Json<TestClockRequest>,
) -> ApiResult<Json<Value>> {
    if let Some(now) = &body.now {
        let at = chrono::DateTime::parse_from_rfc3339(now)
            .map_err(|_| ApiError::BadRequest("now is an RFC 3339 time".into()))?;
        clock::set_now(at.with_timezone(&chrono::Utc));
    }
    crate::jobs::tick(&state).await;
    let now = clock::now();
    Ok(Json(json!({
        "now": now.to_rfc3339(),
        "day": clock::day_string(clock::prompt_day(now)),
    })))
}
