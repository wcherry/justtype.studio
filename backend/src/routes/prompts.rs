//! Daily Prompts: today's prompt and the archive, taking part, writing an
//! answer, the feed of everyone else's, and turning an answer into a project.
//!
//! Who sees what:
//! - Any signed-in writer sees today's prompt and the archive. A day that has
//!   not opened yet is never returned, by any route.
//! - Writing an answer, and reading the feed, need enrollment.
//! - A draft or withdrawn entry is its writer's alone. A submitted one is
//!   readable by every enrolled writer, and stays so after its writer leaves.
//! - A writer can always open their own entries, enrolled or not.
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::NaiveDate;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::AuthUser;
use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::enums::{EntrySort, EntryState};
use crate::domain::filter::Surface;
use crate::domain::{doc, moderation, new_id, prompts as rules, split};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, AuthorCard};

// --- shared helpers ------------------------------------------------------

pub fn today() -> NaiveDate {
    clock::prompt_day(clock::now())
}

/// A day that has opened, or a 404 that does not say whether it exists.
pub fn opened_day(day: &str) -> ApiResult<NaiveDate> {
    let parsed = clock::parse_day(day)
        .ok_or_else(|| ApiError::BadRequest("a day looks like YYYY-MM-DD".into()))?;
    if parsed > today() {
        return Err(ApiError::NotFound(
            "there is no prompt for that day yet".into(),
        ));
    }
    Ok(parsed)
}

pub fn prompt_for(conn: &mut Conn, day: NaiveDate) -> ApiResult<DailyPrompt> {
    daily_prompts::table
        .filter(daily_prompts::day.eq(clock::day_string(day)))
        .select(DailyPrompt::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("there was no prompt that day".into()))
}

pub fn is_enrolled(conn: &mut Conn, user_id: &str) -> ApiResult<bool> {
    let n: i64 = prompt_enrollments::table
        .filter(prompt_enrollments::user_id.eq(user_id))
        .filter(prompt_enrollments::left_at.is_null())
        .count()
        .get_result(conn)?;
    Ok(n > 0)
}

pub fn require_enrolled(conn: &mut Conn, user_id: &str) -> ApiResult<()> {
    if is_enrolled(conn, user_id)? {
        Ok(())
    } else {
        Err(ApiError::Forbidden(
            "join Daily Prompts to write answers and read everyone else's".into(),
        ))
    }
}

/// An entry is late when it was first submitted after its day closed.
pub fn is_late(entry: &PromptEntry, day: NaiveDate) -> bool {
    let closes = clock::day_end(day).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    entry
        .submitted_at
        .as_deref()
        .is_some_and(|at| at >= closes.as_str())
}

/// Prompts lazily get a day of their own if the job runner has not given them
/// one yet (a fresh server, or a missed tick), so today is never empty.
async fn ensure_today(state: &AppState) -> ApiResult<()> {
    crate::jobs::ensure_prompt(state, today(), true).await
}

// --- prompts and the archive ---------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PromptView {
    /// `YYYY-MM-DD`, Pacific.
    pub day: String,
    pub body: String,
    /// When it opened and when it closes.
    pub opens_at: String,
    pub closes_at: String,
    /// The caller's own answer: `none`, or the entry's state.
    pub my_state: String,
    pub my_entry_id: Option<String>,
    /// How many have been submitted, late ones included.
    pub submissions: i64,
}

fn view_prompt(conn: &mut Conn, prompt: &DailyPrompt, user_id: &str) -> ApiResult<PromptView> {
    let day = prompt
        .day
        .as_deref()
        .and_then(clock::parse_day)
        .ok_or_else(|| ApiError::internal("a prompt in the archive has no day"))?;
    let mine: Option<(String, String)> = prompt_entries::table
        .filter(prompt_entries::prompt_id.eq(&prompt.id))
        .filter(prompt_entries::author_id.eq(user_id))
        .select((prompt_entries::id, prompt_entries::state))
        .first(conn)
        .optional()?;
    let submissions: i64 = prompt_entries::table
        .filter(prompt_entries::prompt_id.eq(&prompt.id))
        .filter(prompt_entries::state.eq(EntryState::Submitted.as_str()))
        .count()
        .get_result(conn)?;
    let at =
        |t: chrono::DateTime<chrono::Utc>| t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    Ok(PromptView {
        day: clock::day_string(day),
        body: prompt.body.clone(),
        opens_at: at(clock::day_start(day)),
        closes_at: at(clock::day_end(day)),
        my_state: mine
            .as_ref()
            .map(|m| m.1.clone())
            .unwrap_or_else(|| "none".into()),
        my_entry_id: mine.map(|m| m.0),
        submissions,
    })
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TodayResponse {
    pub prompt: PromptView,
    pub enrolled: bool,
}

#[utoipa::path(
    get,
    path = "/api/prompts/today",
    tag = "prompts",
    responses(
        (status = 200, description = "Today's prompt", body = TodayResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn today_prompt(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<TodayResponse>> {
    ensure_today(&state).await?;
    let response = blocking(&state.pool, move |conn| {
        let prompt = prompt_for(conn, today())?;
        Ok(TodayResponse {
            prompt: view_prompt(conn, &prompt, &user.id)?,
            enrolled: is_enrolled(conn, &user.id)?,
        })
    })
    .await?;
    Ok(Json(response))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveQuery {
    /// Days before this one (`YYYY-MM-DD`). Unset, the days before today.
    #[serde(default)]
    pub before: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArchivePage {
    pub prompts: Vec<PromptView>,
    /// Pass as `before` for the page after this one; unset at the first day.
    pub next_before: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/prompts",
    tag = "prompts",
    params(ArchiveQuery),
    responses(
        (status = 200, description = "Past prompts, newest first", body = ArchivePage),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn archive(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ArchiveQuery>,
) -> ApiResult<Json<ArchivePage>> {
    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    // Never past today: a future day stays hidden however `before` is set.
    let today_s = clock::day_string(today());
    let before = match &query.before {
        Some(b) => {
            clock::parse_day(b)
                .ok_or_else(|| ApiError::BadRequest("before looks like YYYY-MM-DD".into()))?;
            b.clone().min(today_s)
        }
        None => today_s,
    };
    let page = blocking(&state.pool, move |conn| {
        let rows: Vec<DailyPrompt> = daily_prompts::table
            .filter(daily_prompts::day.lt(&before))
            .order(daily_prompts::day.desc())
            .limit(limit + 1)
            .select(DailyPrompt::as_select())
            .load(conn)?;
        let more = rows.len() as i64 > limit;
        let mut prompts = Vec::new();
        for row in rows.iter().take(limit as usize) {
            prompts.push(view_prompt(conn, row, &user.id)?);
        }
        Ok(ArchivePage {
            next_before: more
                .then(|| prompts.last().map(|p| p.day.clone()))
                .flatten(),
            prompts,
        })
    })
    .await?;
    Ok(Json(page))
}

#[utoipa::path(
    get,
    path = "/api/prompts/{day}",
    tag = "prompts",
    params(("day" = String, Path, description = "YYYY-MM-DD")),
    responses(
        (status = 200, description = "That day's prompt", body = PromptView),
        (status = 404, description = "No prompt that day, or not open yet", body = ErrorEnvelope),
    )
)]
pub async fn one_day(
    State(state): State<AppState>,
    user: AuthUser,
    Path(day): Path<String>,
) -> ApiResult<Json<PromptView>> {
    let day = opened_day(&day)?;
    if day == today() {
        ensure_today(&state).await?;
    }
    let view = blocking(&state.pool, move |conn| {
        let prompt = prompt_for(conn, day)?;
        view_prompt(conn, &prompt, &user.id)
    })
    .await?;
    Ok(Json(view))
}

// --- taking part ---------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentView {
    pub enrolled: bool,
    pub enrolled_at: Option<String>,
    pub email_enabled: bool,
    /// `HH:MM` in `email_timezone`.
    pub email_local_time: String,
    pub email_timezone: String,
    /// Email can only be turned on for a confirmed address.
    pub email_verified: bool,
    /// When today's prompt opens (and so when it closes), as `HH:MM` in
    /// `email_timezone`, for the note beside the time.
    pub opens_at_local: String,
}

fn view_enrollment(row: Option<PromptEnrollment>, verified: bool) -> EnrollmentView {
    let (tz_name, time) = row
        .as_ref()
        .map(|r| (r.email_timezone.clone(), r.email_local_time.clone()))
        .unwrap_or_else(|| ("America/Los_Angeles".into(), "07:00".into()));
    let tz: chrono_tz::Tz = tz_name.parse().unwrap_or(chrono_tz::America::Los_Angeles);
    EnrollmentView {
        enrolled: row.as_ref().is_some_and(|r| r.left_at.is_none()),
        enrolled_at: row
            .as_ref()
            .filter(|r| r.left_at.is_none())
            .map(|r| r.enrolled_at.clone()),
        email_enabled: row
            .as_ref()
            .is_some_and(|r| r.email_enabled && r.left_at.is_none()),
        email_local_time: time,
        email_timezone: tz_name,
        email_verified: verified,
        opens_at_local: rules::opens_local(today(), tz).format("%H:%M").to_string(),
    }
}

fn load_enrollment(conn: &mut Conn, user: &AuthUser) -> ApiResult<EnrollmentView> {
    let row: Option<PromptEnrollment> = prompt_enrollments::table
        .find(&user.id)
        .select(PromptEnrollment::as_select())
        .first(conn)
        .optional()?;
    Ok(view_enrollment(row, user.email_verified))
}

fn check_tz(name: &str) -> ApiResult<chrono_tz::Tz> {
    name.parse().map_err(|_| {
        ApiError::BadRequest(format!(
            "`{name}` is not a timezone; use a name such as Europe/London"
        ))
    })
}

#[utoipa::path(
    get,
    path = "/api/prompts/enrollment",
    tag = "prompts",
    responses(
        (status = 200, description = "Whether the caller takes part, and how", body = EnrollmentView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_enrollment(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<EnrollmentView>> {
    Ok(Json(
        blocking(&state.pool, move |conn| load_enrollment(conn, &user)).await?,
    ))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JoinRequest {
    /// The writer's timezone (IANA), from their browser. Sets the default
    /// time for the email, which stays off until they turn it on.
    #[serde(default)]
    pub timezone: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/prompts/enrollment",
    tag = "prompts",
    request_body = JoinRequest,
    responses(
        (status = 200, description = "Joined", body = EnrollmentView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn join(
    State(state): State<AppState>,
    user: AuthUser,
    body: Option<Json<JoinRequest>>,
) -> ApiResult<Json<EnrollmentView>> {
    let body = body.map(|b| b.0).unwrap_or_default();
    let tz = match &body.timezone {
        Some(name) => check_tz(name)?,
        None => chrono_tz::America::Los_Angeles,
    };
    let view = blocking(&state.pool, move |conn| {
        let now = clock::now_iso();
        let existing: Option<PromptEnrollment> = prompt_enrollments::table
            .find(&user.id)
            .select(PromptEnrollment::as_select())
            .first(conn)
            .optional()?;
        match existing {
            // Back again, with the preferences they left with; the email
            // stays off until they turn it on again.
            Some(row) if row.left_at.is_some() => {
                diesel::update(prompt_enrollments::table.find(&user.id))
                    .set((
                        prompt_enrollments::left_at.eq(None::<String>),
                        prompt_enrollments::enrolled_at.eq(&now),
                        prompt_enrollments::updated_at.eq(&now),
                    ))
                    .execute(conn)?;
            }
            Some(_) => {}
            None => {
                diesel::insert_into(prompt_enrollments::table)
                    .values(&NewPromptEnrollment {
                        user_id: user.id.clone(),
                        enrolled_at: now.clone(),
                        left_at: None,
                        email_enabled: false,
                        email_local_time: rules::default_send_time(tz, today())
                            .format("%H:%M")
                            .to_string(),
                        email_timezone: tz.name().to_string(),
                        unsubscribe_token: crate::auth::generate_link_token(),
                        updated_at: now,
                    })
                    .execute(conn)?;
            }
        }
        load_enrollment(conn, &user)
    })
    .await?;
    Ok(Json(view))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEnrollmentRequest {
    #[serde(default)]
    pub email_enabled: Option<bool>,
    /// `HH:MM`.
    #[serde(default)]
    pub email_local_time: Option<String>,
    /// An IANA name.
    #[serde(default)]
    pub email_timezone: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/api/prompts/enrollment",
    tag = "prompts",
    request_body = UpdateEnrollmentRequest,
    responses(
        (status = 200, description = "Saved", body = EnrollmentView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update_enrollment(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UpdateEnrollmentRequest>,
) -> ApiResult<Json<EnrollmentView>> {
    if let Some(time) = &body.email_local_time {
        clock::parse_time(time)
            .ok_or_else(|| ApiError::BadRequest("the time looks like HH:MM".into()))?;
    }
    if let Some(tz) = &body.email_timezone {
        check_tz(tz)?;
    }
    if body.email_enabled == Some(true) && !user.email_verified {
        return Err(ApiError::Forbidden(
            "confirm your address first: the prompt email only goes to a confirmed one".into(),
        ));
    }
    let view = blocking(&state.pool, move |conn| {
        require_enrolled(conn, &user.id)?;
        let target = prompt_enrollments::table.find(&user.id);
        let now = clock::now_iso();
        if let Some(on) = body.email_enabled {
            diesel::update(target)
                .set(prompt_enrollments::email_enabled.eq(on))
                .execute(conn)?;
        }
        if let Some(time) = &body.email_local_time {
            diesel::update(target)
                .set(prompt_enrollments::email_local_time.eq(time))
                .execute(conn)?;
        }
        if let Some(tz) = &body.email_timezone {
            diesel::update(target)
                .set(prompt_enrollments::email_timezone.eq(tz))
                .execute(conn)?;
        }
        diesel::update(target)
            .set(prompt_enrollments::updated_at.eq(now))
            .execute(conn)?;
        load_enrollment(conn, &user)
    })
    .await?;
    Ok(Json(view))
}

#[utoipa::path(
    delete,
    path = "/api/prompts/enrollment",
    tag = "prompts",
    responses(
        (status = 200, description = "Left. Submitted answers and critiques stay where they are.", body = EnrollmentView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn leave(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<EnrollmentView>> {
    let view = blocking(&state.pool, move |conn| {
        let now = clock::now_iso();
        diesel::update(prompt_enrollments::table.find(&user.id))
            .set((
                prompt_enrollments::left_at.eq(Some(&now)),
                prompt_enrollments::email_enabled.eq(false),
                prompt_enrollments::updated_at.eq(&now),
            ))
            .execute(conn)?;
        load_enrollment(conn, &user)
    })
    .await?;
    Ok(Json(view))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct UnsubscribeQuery {
    pub token: String,
}

/// One click, signed in or not: the link in the email's footer (through the
/// web page) and mail clients' `List-Unsubscribe-Post` both land here. Turns
/// the email off and nothing else.
#[utoipa::path(
    post,
    path = "/api/prompts/unsubscribe",
    tag = "prompts",
    params(UnsubscribeQuery),
    responses(
        (status = 200, description = "The email is off", body = serde_json::Value),
        (status = 404, description = "That link is not one of ours", body = ErrorEnvelope),
    )
)]
pub async fn unsubscribe(
    State(state): State<AppState>,
    Query(query): Query<UnsubscribeQuery>,
) -> ApiResult<Json<Value>> {
    blocking(&state.pool, move |conn| {
        let changed = diesel::update(
            prompt_enrollments::table
                .filter(prompt_enrollments::unsubscribe_token.eq(&query.token)),
        )
        .set((
            prompt_enrollments::email_enabled.eq(false),
            prompt_enrollments::updated_at.eq(clock::now_iso()),
        ))
        .execute(conn)?;
        if changed == 0 {
            return Err(ApiError::NotFound(
                "that unsubscribe link is not one we sent".into(),
            ));
        }
        Ok(())
    })
    .await?;
    Ok(Json(json!({ "unsubscribed": true })))
}

// --- the writer's own answer ---------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkLink {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PromptEntryView {
    #[serde(flatten)]
    pub entry: PromptEntry,
    pub day: String,
    pub prompt: String,
    /// The answer, as a ProseMirror document.
    #[schema(value_type = Object)]
    pub content: Value,
    pub author: AuthorCard,
    pub late: bool,
    pub critique_count: i64,
    pub mine: bool,
    /// The last project made from it, if it still exists.
    pub converted_work: Option<WorkLink>,
}

pub fn view_entry(conn: &mut Conn, entry: PromptEntry, viewer: &str) -> ApiResult<PromptEntryView> {
    let prompt: DailyPrompt = daily_prompts::table
        .find(&entry.prompt_id)
        .select(DailyPrompt::as_select())
        .first(conn)?;
    let day = prompt
        .day
        .as_deref()
        .and_then(clock::parse_day)
        .unwrap_or_else(today);
    let author = support::author_cards(conn, std::slice::from_ref(&entry.author_id))?
        .remove(&entry.author_id)
        .unwrap_or(AuthorCard {
            id: entry.author_id.clone(),
            display_name: "Unknown".into(),
        });
    let critique_count: i64 = prompt_critiques::table
        .filter(prompt_critiques::entry_id.eq(&entry.id))
        .filter(prompt_critiques::deleted_at.is_null())
        .count()
        .get_result(conn)?;
    let converted_work = match &entry.converted_work_id {
        Some(id) if entry.author_id == viewer => works::table
            .find(id)
            .select(works::title)
            .first::<String>(conn)
            .optional()?
            .map(|title| WorkLink {
                id: id.clone(),
                title,
            }),
        _ => None,
    };
    Ok(PromptEntryView {
        day: clock::day_string(day),
        prompt: prompt.body,
        content: serde_json::from_str(&entry.body).unwrap_or_else(|_| doc::empty_doc()),
        author,
        late: is_late(&entry, day),
        critique_count,
        mine: entry.author_id == viewer,
        converted_work,
        entry,
    })
}

fn own_entry(conn: &mut Conn, prompt_id: &str, user_id: &str) -> ApiResult<Option<PromptEntry>> {
    Ok(prompt_entries::table
        .filter(prompt_entries::prompt_id.eq(prompt_id))
        .filter(prompt_entries::author_id.eq(user_id))
        .select(PromptEntry::as_select())
        .first(conn)
        .optional()?)
}

#[utoipa::path(
    get,
    path = "/api/prompts/{day}/entry",
    tag = "prompts",
    params(("day" = String, Path)),
    responses(
        (status = 200, description = "The caller's answer", body = PromptEntryView),
        (status = 404, description = "Not started", body = ErrorEnvelope),
    )
)]
pub async fn get_entry(
    State(state): State<AppState>,
    user: AuthUser,
    Path(day): Path<String>,
) -> ApiResult<Json<PromptEntryView>> {
    let day = opened_day(&day)?;
    let view = blocking(&state.pool, move |conn| {
        let prompt = prompt_for(conn, day)?;
        let entry = own_entry(conn, &prompt.id, &user.id)?.ok_or_else(|| {
            ApiError::NotFound("you have not started an answer to this one".into())
        })?;
        view_entry(conn, entry, &user.id)
    })
    .await?;
    Ok(Json(view))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveEntryRequest {
    #[schema(value_type = Object)]
    pub content: Value,
    /// The hash the edit was made on top of; a save against something newer
    /// is refused with 409, as scene saves are.
    #[serde(default)]
    pub base_hash: Option<String>,
    /// The writer's own date, for their daily goal.
    #[serde(default)]
    pub local_day: Option<String>,
}

#[utoipa::path(
    put,
    path = "/api/prompts/{day}/entry",
    tag = "prompts",
    params(("day" = String, Path)),
    request_body = SaveEntryRequest,
    responses(
        (status = 200, description = "Saved", body = PromptEntryView),
        (status = 409, description = "Changed somewhere else since", body = ErrorEnvelope),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn save_entry(
    State(state): State<AppState>,
    user: AuthUser,
    Path(day): Path<String>,
    Json(body): Json<SaveEntryRequest>,
) -> ApiResult<Json<PromptEntryView>> {
    let day = opened_day(&day)?;
    doc::validate(&body.content).map_err(ApiError::BadRequest)?;
    if day == today() {
        ensure_today(&state).await?;
    }
    // An edit to a submitted answer is public text again, so it is screened
    // like the submission was. A draft is private and is not.
    let screen = {
        let uid = user.id.clone();
        let submitted = blocking(&state.pool, move |conn| {
            let prompt = prompt_for(conn, day)?;
            Ok(own_entry(conn, &prompt.id, &uid)?
                .is_some_and(|e| e.state == EntryState::Submitted.as_str()))
        })
        .await?;
        if submitted {
            let text = doc::plain_text(&body.content);
            Some(moderation::screen(&state, Surface::Entry, &user.id, &text).await?)
        } else {
            None
        }
    };
    let view = blocking(&state.pool, move |conn| {
        require_enrolled(conn, &user.id)?;
        let prompt = prompt_for(conn, day)?;
        let hash = doc::content_hash(&body.content);
        let words = doc::word_count(&body.content);
        let json = serde_json::to_string(&body.content)?;
        let now = clock::now_iso();
        conn.transaction::<_, ApiError, _>(|conn| {
            let existing = own_entry(conn, &prompt.id, &user.id)?;
            let previous_words = existing.as_ref().map_or(0, |e| e.word_count);
            match &existing {
                Some(entry) => {
                    if let Some(base) = &body.base_hash {
                        if base != &entry.content_hash && hash != entry.content_hash {
                            return Err(ApiError::ConflictWith {
                                message: "this answer changed somewhere else since you opened it"
                                    .into(),
                                details: json!({
                                    "sceneId": entry.id,
                                    "currentHash": entry.content_hash,
                                    "currentWordCount": entry.word_count,
                                    "updatedAt": entry.updated_at,
                                    "yourHash": hash,
                                }),
                            });
                        }
                    }
                    if entry.content_hash != hash {
                        let edited =
                            (entry.state == EntryState::Submitted.as_str()).then(|| now.clone());
                        diesel::update(prompt_entries::table.find(&entry.id))
                            .set((
                                prompt_entries::body.eq(&json),
                                prompt_entries::content_hash.eq(&hash),
                                prompt_entries::word_count.eq(words),
                                prompt_entries::updated_at.eq(&now),
                            ))
                            .execute(conn)?;
                        if let Some(at) = edited {
                            diesel::update(prompt_entries::table.find(&entry.id))
                                .set(prompt_entries::edited_at.eq(Some(at)))
                                .execute(conn)?;
                            if let Some(screen) = &screen {
                                let provider = moderation::provider_name(conn)?;
                                moderation::record(
                                    conn, screen, "entry", &entry.id, &user.id, &provider,
                                )?;
                            }
                        }
                    }
                }
                None => {
                    diesel::insert_into(prompt_entries::table)
                        .values(&NewPromptEntry {
                            id: new_id(),
                            prompt_id: prompt.id.clone(),
                            author_id: user.id.clone(),
                            body: json.clone(),
                            content_hash: hash.clone(),
                            word_count: words,
                            state: EntryState::Draft.as_str().into(),
                            submitted_at: None,
                            edited_at: None,
                            converted_work_id: None,
                            created_at: now.clone(),
                            updated_at: now.clone(),
                        })
                        .execute(conn)?;
                }
            }
            // Words written here count toward the day's goal, as a scene's do.
            if let Some(local) = &body.local_day {
                if words != previous_words || existing.is_none() {
                    super::goals::record_save(conn, &user.id, local, words - previous_words)?;
                }
            }
            Ok(())
        })?;
        let entry = own_entry(conn, &prompt.id, &user.id)?.expect("just saved");
        view_entry(conn, entry, &user.id)
    })
    .await?;
    Ok(Json(view))
}

async fn transition(
    state: &AppState,
    user: AuthUser,
    day: String,
    to: EntryState,
) -> ApiResult<PromptEntryView> {
    let day = opened_day(&day)?;
    // Only an answer being made public is screened: until it is submitted it
    // is private, and private writing is never sent anywhere.
    let screen = if to == EntryState::Submitted {
        let uid = user.id.clone();
        let text = blocking(&state.pool, move |conn| {
            let prompt = prompt_for(conn, day)?;
            Ok(own_entry(conn, &prompt.id, &uid)?
                .map(|e| doc::plain_text(&serde_json::from_str(&e.body).unwrap_or(Value::Null))))
        })
        .await?;
        match text {
            Some(text) => Some(moderation::screen(state, Surface::Entry, &user.id, &text).await?),
            None => None,
        }
    } else {
        None
    };
    blocking(&state.pool, move |conn| {
        require_enrolled(conn, &user.id)?;
        let prompt = prompt_for(conn, day)?;
        let entry = own_entry(conn, &prompt.id, &user.id)?.ok_or_else(|| {
            ApiError::NotFound("you have not started an answer to this one".into())
        })?;
        let now = clock::now_iso();
        match to {
            EntryState::Submitted => {
                if entry.word_count == 0 {
                    return Err(ApiError::BadRequest(
                        "write something before you submit it".into(),
                    ));
                }
                diesel::update(prompt_entries::table.find(&entry.id))
                    .set((
                        prompt_entries::state.eq(to.as_str()),
                        // The first submission is the one that counts for
                        // lateness; withdrawing and submitting again does not
                        // move it.
                        prompt_entries::submitted_at
                            .eq(entry.submitted_at.clone().or(Some(now.clone()))),
                        prompt_entries::updated_at.eq(&now),
                    ))
                    .execute(conn)?;
                if let Some(screen) = &screen {
                    let provider = moderation::provider_name(conn)?;
                    moderation::record(conn, screen, "entry", &entry.id, &user.id, &provider)?;
                }
            }
            EntryState::Withdrawn => {
                if entry.state != EntryState::Submitted.as_str() {
                    return Err(ApiError::BadRequest(
                        "only a submitted answer can be withdrawn".into(),
                    ));
                }
                diesel::update(prompt_entries::table.find(&entry.id))
                    .set((
                        prompt_entries::state.eq(to.as_str()),
                        prompt_entries::updated_at.eq(&now),
                    ))
                    .execute(conn)?;
            }
            EntryState::Draft => unreachable!("not a transition"),
        }
        let entry = own_entry(conn, &prompt.id, &user.id)?.expect("still there");
        view_entry(conn, entry, &user.id)
    })
    .await
}

#[utoipa::path(
    post,
    path = "/api/prompts/{day}/entry/submit",
    tag = "prompts",
    params(("day" = String, Path)),
    responses(
        (status = 200, description = "Readable by every enrolled writer", body = PromptEntryView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn submit(
    State(state): State<AppState>,
    user: AuthUser,
    Path(day): Path<String>,
) -> ApiResult<Json<PromptEntryView>> {
    Ok(Json(
        transition(&state, user, day, EntryState::Submitted).await?,
    ))
}

#[utoipa::path(
    post,
    path = "/api/prompts/{day}/entry/withdraw",
    tag = "prompts",
    params(("day" = String, Path)),
    responses(
        (status = 200, description = "Private again; its critiques are kept", body = PromptEntryView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn withdraw(
    State(state): State<AppState>,
    user: AuthUser,
    Path(day): Path<String>,
) -> ApiResult<Json<PromptEntryView>> {
    Ok(Json(
        transition(&state, user, day, EntryState::Withdrawn).await?,
    ))
}

/// Deletes an entry with everything about it: its critiques (by cascade) and,
/// in the same transaction, every inbox item about it or them.
pub fn delete_entry_rows(conn: &mut Conn, entry_id: &str) -> ApiResult<()> {
    conn.transaction::<_, ApiError, _>(|conn| {
        super::inbox::forget_context(conn, "prompt_entry", entry_id)?;
        super::inbox::forget_subject(conn, "prompt_entry", entry_id)?;
        diesel::delete(prompt_entries::table.find(entry_id)).execute(conn)?;
        Ok(())
    })
}

#[utoipa::path(
    delete,
    path = "/api/prompts/{day}/entry",
    tag = "prompts",
    params(("day" = String, Path)),
    responses(
        (status = 200, description = "Gone, with all its critiques", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_entry(
    State(state): State<AppState>,
    user: AuthUser,
    Path(day): Path<String>,
) -> ApiResult<Json<Value>> {
    let day = opened_day(&day)?;
    blocking(&state.pool, move |conn| {
        let prompt = prompt_for(conn, day)?;
        let entry = own_entry(conn, &prompt.id, &user.id)?
            .ok_or_else(|| ApiError::NotFound("there is no answer to delete".into()))?;
        delete_entry_rows(conn, &entry.id)
    })
    .await?;
    Ok(Json(json!({ "deleted": true })))
}

// --- everyone else's -----------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryCard {
    pub id: String,
    pub day: String,
    pub author: AuthorCard,
    /// The first few lines.
    pub excerpt: String,
    pub word_count: i32,
    pub critique_count: i64,
    pub submitted_at: Option<String>,
    pub edited_at: Option<String>,
    pub late: bool,
    pub mine: bool,
    /// `held` (awaiting review) or `removed`, on the writer's own answer.
    pub moderated: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryPage {
    pub entries: Vec<EntryCard>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct FeedQuery {
    /// `newest` (the default) or `fewest_critiques`.
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Where a card sorts: by critiques (only for that order), then newest.
fn sort_key(sort: EntrySort, card: &EntryCard) -> String {
    let when = card.submitted_at.clone().unwrap_or_default();
    match sort {
        EntrySort::Newest => format!("{when}~{}", card.id),
        EntrySort::FewestCritiques => format!("{:010}~{when}~{}", card.critique_count, card.id),
    }
}

#[utoipa::path(
    get,
    path = "/api/prompts/{day}/entries",
    tag = "prompts",
    params(("day" = String, Path), FeedQuery),
    responses(
        (status = 200, description = "The day's submitted answers, late ones too", body = EntryPage),
        (status = 403, description = "Not enrolled", body = ErrorEnvelope),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn feed(
    State(state): State<AppState>,
    user: AuthUser,
    Path(day): Path<String>,
    Query(query): Query<FeedQuery>,
) -> ApiResult<Json<EntryPage>> {
    let day = opened_day(&day)?;
    let sort = match query.sort.as_deref().unwrap_or("newest") {
        "newest" => EntrySort::Newest,
        "fewest_critiques" => EntrySort::FewestCritiques,
        other => {
            return Err(ApiError::BadRequest(format!(
                "sort must be one of {}, not `{other}`",
                EntrySort::options()
            )))
        }
    };
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let page = blocking(&state.pool, move |conn| {
        require_enrolled(conn, &user.id)?;
        let Ok(prompt) = prompt_for(conn, day) else {
            return Ok(EntryPage {
                entries: Vec::new(),
                next_cursor: None,
            });
        };
        // Held and removed answers, and answers by an account that is hidden,
        // are out of everyone else's feed; a writer still sees their own.
        let hidden = crate::domain::restrictions::hidden_users(conn)?;
        let rows: Vec<PromptEntry> = prompt_entries::table
            .filter(prompt_entries::prompt_id.eq(&prompt.id))
            .filter(prompt_entries::state.eq(EntryState::Submitted.as_str()))
            .filter(
                prompt_entries::moderation_state
                    .eq("visible")
                    .or(prompt_entries::author_id.eq(&user.id)),
            )
            .filter(prompt_entries::author_id.ne_all(&hidden))
            .select(PromptEntry::as_select())
            .load(conn)?;
        let ids: Vec<String> = rows.iter().map(|e| e.id.clone()).collect();
        let counts: Vec<(String, i64)> = prompt_critiques::table
            .filter(prompt_critiques::entry_id.eq_any(&ids))
            .filter(prompt_critiques::deleted_at.is_null())
            .group_by(prompt_critiques::entry_id)
            .select((prompt_critiques::entry_id, diesel::dsl::count_star()))
            .load(conn)?;
        let counts: std::collections::HashMap<String, i64> = counts.into_iter().collect();
        let authors: Vec<String> = rows.iter().map(|e| e.author_id.clone()).collect();
        let cards_by = support::author_cards(conn, &authors)?;
        let mut cards: Vec<EntryCard> = rows
            .into_iter()
            .map(|e| {
                let content: Value =
                    serde_json::from_str(&e.body).unwrap_or_else(|_| doc::empty_doc());
                EntryCard {
                    day: clock::day_string(day),
                    author: cards_by.get(&e.author_id).cloned().unwrap_or(AuthorCard {
                        id: e.author_id.clone(),
                        display_name: "Unknown".into(),
                    }),
                    excerpt: doc::excerpt(&content, 280),
                    word_count: e.word_count,
                    critique_count: counts.get(&e.id).copied().unwrap_or(0),
                    late: is_late(&e, day),
                    mine: e.author_id == user.id,
                    moderated: (e.moderation_state != "visible")
                        .then(|| e.moderation_state.clone()),
                    submitted_at: e.submitted_at,
                    edited_at: e.edited_at,
                    id: e.id,
                }
            })
            .collect();
        match sort {
            EntrySort::Newest => cards.sort_by_key(|c| std::cmp::Reverse(sort_key(sort, c))),
            EntrySort::FewestCritiques => cards.sort_by(|a, b| {
                a.critique_count
                    .cmp(&b.critique_count)
                    .then_with(|| b.submitted_at.cmp(&a.submitted_at))
                    .then_with(|| b.id.cmp(&a.id))
            }),
        }
        // The cursor is the key of the last card shown: the page after it is
        // whatever comes after that key in the same order, however the list
        // has grown since.
        let start = match &query.cursor {
            Some(cursor) => cards
                .iter()
                .position(|c| &sort_key(sort, c) == cursor)
                .map(|i| i + 1)
                .unwrap_or(0),
            None => 0,
        };
        let rest: Vec<EntryCard> = cards.into_iter().skip(start).collect();
        let more = rest.len() > limit;
        let entries: Vec<EntryCard> = rest.into_iter().take(limit).collect();
        Ok(EntryPage {
            next_cursor: more
                .then(|| entries.last().map(|c| sort_key(sort, c)))
                .flatten(),
            entries,
        })
    })
    .await?;
    Ok(Json(page))
}

/// The entry, if the caller may read it: their own in any state, anyone's
/// submitted one while they are enrolled.
pub fn readable_entry(conn: &mut Conn, entry_id: &str, user_id: &str) -> ApiResult<PromptEntry> {
    let entry: PromptEntry = prompt_entries::table
        .find(entry_id)
        .select(PromptEntry::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("that answer is not here".into()))?;
    if entry.author_id == user_id {
        return Ok(entry);
    }
    require_enrolled(conn, user_id)?;
    if entry.state != EntryState::Submitted.as_str() {
        return Err(ApiError::NotFound("that answer is not here".into()));
    }
    // Held or removed by moderation, or by an account that is hidden: it is
    // not there for anyone but staff.
    let hidden = crate::domain::restrictions::hidden_users(conn)?;
    let staff = moderation::is_staff(conn, Some(user_id))?;
    if !staff && (entry.moderation_state != "visible" || hidden.contains(&entry.author_id)) {
        return Err(ApiError::NotFound("that answer is not here".into()));
    }
    Ok(entry)
}

#[utoipa::path(
    get,
    path = "/api/prompt-entries/{entry_id}",
    tag = "prompts",
    params(("entry_id" = String, Path)),
    responses(
        (status = 200, description = "The whole answer", body = PromptEntryView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(entry_id): Path<String>,
) -> ApiResult<Json<PromptEntryView>> {
    let view = blocking(&state.pool, move |conn| {
        let entry = readable_entry(conn, &entry_id, &user.id)?;
        view_entry(conn, entry, &user.id)
    })
    .await?;
    Ok(Json(view))
}

// --- into a project ------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScenePreview {
    pub first_line: String,
    pub word_count: i32,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToWorkPreview {
    /// A title from the prompt, for the dialog to offer.
    pub title: String,
    pub scenes: Vec<ScenePreview>,
    /// Whether a blank paragraph separated scenes, from the writer's setting.
    pub blank_paragraph_splits: bool,
}

struct Conversion {
    prompt: DailyPrompt,
    day: String,
    scenes: Vec<Value>,
    blank: bool,
}

fn convert(conn: &mut Conn, entry_id: &str, user_id: &str) -> ApiResult<Conversion> {
    let entry: PromptEntry = prompt_entries::table
        .find(entry_id)
        .filter(prompt_entries::author_id.eq(user_id))
        .select(PromptEntry::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("only your own answer can become a project".into()))?;
    let prompt: DailyPrompt = daily_prompts::table
        .find(&entry.prompt_id)
        .select(DailyPrompt::as_select())
        .first(conn)?;
    let blank: bool = users::table
        .find(user_id)
        .select(users::double_enter_splits_scenes)
        .first(conn)?;
    let content: Value = serde_json::from_str(&entry.body).unwrap_or_else(|_| doc::empty_doc());
    Ok(Conversion {
        day: prompt.day.clone().unwrap_or_default(),
        prompt,
        scenes: split::split_scenes(&content, blank),
        blank,
    })
}

#[utoipa::path(
    post,
    path = "/api/prompt-entries/{entry_id}/to-work/preview",
    tag = "prompts",
    params(("entry_id" = String, Path)),
    responses(
        (status = 200, description = "The scenes it would make; nothing is created", body = ToWorkPreview),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn to_work_preview(
    State(state): State<AppState>,
    user: AuthUser,
    Path(entry_id): Path<String>,
) -> ApiResult<Json<ToWorkPreview>> {
    let c = blocking(&state.pool, move |conn| convert(conn, &entry_id, &user.id)).await?;
    Ok(Json(ToWorkPreview {
        title: rules::title_from_prompt(&c.prompt.body),
        scenes: c
            .scenes
            .iter()
            .map(|s| ScenePreview {
                first_line: split::first_line(s, 80),
                word_count: doc::word_count(s),
            })
            .collect(),
        blank_paragraph_splits: c.blank,
    }))
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToWorkRequest {
    /// Unset, the title the preview offered.
    #[serde(default)]
    pub title: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/prompt-entries/{entry_id}/to-work",
    tag = "prompts",
    params(("entry_id" = String, Path)),
    request_body = ToWorkRequest,
    responses(
        (status = 200, description = "The new work, a copy; the answer is unchanged", body = super::works::OutlineResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn to_work(
    State(state): State<AppState>,
    user: AuthUser,
    Path(entry_id): Path<String>,
    body: Option<Json<ToWorkRequest>>,
) -> ApiResult<Json<super::works::OutlineResponse>> {
    use crate::export::project::{
        Project, ProjectChapter, ProjectEntry, ProjectScene, ProjectWork,
    };
    let body = body.map(|b| b.0).unwrap_or_default();
    let (c, owner) = {
        let entry_id = entry_id.clone();
        let user_id = user.id.clone();
        blocking(&state.pool, move |conn| {
            Ok((convert(conn, &entry_id, &user_id)?, user_id))
        })
        .await?
    };
    if c.scenes.is_empty() {
        return Err(ApiError::BadRequest(
            "there is no text in this answer to make a project of".into(),
        ));
    }
    let title = body
        .title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| rules::title_from_prompt(&c.prompt.body));
    if title.chars().count() > 200 {
        return Err(ApiError::BadRequest(
            "a title is at most 200 characters".into(),
        ));
    }
    let project = Project {
        format: "justtype-project".into(),
        format_version: 1,
        work: ProjectWork {
            title,
            kind: Some("short_story".into()),
            ..Default::default()
        },
        chapters: vec![ProjectChapter {
            scenes: c
                .scenes
                .into_iter()
                .map(|content| ProjectScene {
                    content,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }],
        notebook: vec![ProjectEntry {
            id: "prompt".into(),
            kind: "note".into(),
            name: format!("Daily prompt — {}", c.day),
            body: Some(c.prompt.body.clone()),
            ..Default::default()
        }],
        ..Default::default()
    };
    // The same path a project import takes: one transaction for the rows,
    // then the scene files. Not a save, so no words reach the day's goal.
    let outline = super::project::create_from(&state, user, project).await?;
    let work_id = outline.work.id.clone();
    blocking(&state.pool, move |conn| {
        diesel::update(
            prompt_entries::table
                .find(&entry_id)
                .filter(prompt_entries::author_id.eq(&owner)),
        )
        .set(prompt_entries::converted_work_id.eq(Some(work_id)))
        .execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(outline))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lateness_is_the_first_submission_after_the_day_closed() {
        let day = clock::parse_day("2026-10-12").unwrap();
        let closes = clock::day_end(day);
        let at = |t: chrono::DateTime<chrono::Utc>| {
            t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        };
        let mut e = PromptEntry {
            id: "e".into(),
            prompt_id: "p".into(),
            author_id: "a".into(),
            body: "{}".into(),
            content_hash: String::new(),
            word_count: 1,
            state: "submitted".into(),
            submitted_at: Some(at(closes - chrono::Duration::seconds(1))),
            edited_at: None,
            converted_work_id: None,
            created_at: String::new(),
            updated_at: String::new(),
            moderation_state: "visible".into(),
        };
        assert!(!is_late(&e, day));
        e.submitted_at = Some(at(closes));
        assert!(is_late(&e, day));
        e.submitted_at = None;
        assert!(!is_late(&e, day));
    }
}
