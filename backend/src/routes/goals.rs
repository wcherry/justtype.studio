//! Writing goals: what the writer did today, how long they have kept it up, and
//! what they are aiming at.
//!
//! The day is the writer's *local* date, supplied by the client. Only the client
//! knows what day it is where the writer is, and a UTC day would break a streak
//! for anyone writing in the evening west of Greenwich — which is precisely the
//! writer this feature exists to encourage.
use axum::extract::{Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::db::models::{NewWritingDay, User, WritingDay};
use crate::db::schema::{users, writing_days};
use crate::db::{blocking, Conn};
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

/// `YYYY-MM-DD`, and nothing else: this reaches a `LIKE`-free equality filter,
/// but it is also displayed and compared, so a malformed one would be silent.
fn valid_day(day: &str) -> bool {
    let bytes = day.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
}

/// Adds a save to the writer's day. Called from the scene save path, inside the
/// same blocking closure, so a recorded save and the word count it came from
/// cannot disagree.
pub fn record_save(conn: &mut Conn, user_id: &str, day: &str, delta: i32) -> ApiResult<()> {
    if !valid_day(day) {
        return Err(ApiError::BadRequest(format!(
            "localDay must look like YYYY-MM-DD, got `{day}`"
        )));
    }
    let now = now_iso();
    let existing: Option<WritingDay> = writing_days::table
        .filter(writing_days::user_id.eq(user_id))
        .filter(writing_days::day.eq(day))
        .select(WritingDay::as_select())
        .first(conn)
        .optional()?;

    match existing {
        Some(row) => {
            diesel::update(writing_days::table.find(&row.id))
                .set((
                    writing_days::net_words.eq(row.net_words + delta),
                    writing_days::saves.eq(row.saves + 1),
                    writing_days::updated_at.eq(&now),
                ))
                .execute(conn)?;
        }
        None => {
            diesel::insert_into(writing_days::table)
                .values(&NewWritingDay {
                    id: new_id(),
                    user_id: user_id.to_string(),
                    day: day.to_string(),
                    net_words: delta,
                    saves: 1,
                    created_at: now.clone(),
                    updated_at: now,
                })
                .execute(conn)?;
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WritingQuery {
    /// The writer's local date, so "today" means what they think it means.
    pub today: String,
    /// How many days of history to return. Default 30.
    #[serde(default)]
    pub days: Option<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WritingSummary {
    pub today: String,
    /// Net words on `today`, which may be negative on a day spent cutting.
    pub words_today: i32,
    pub saves_today: i32,
    /// The writer's target, if they have set one.
    pub daily_word_goal: Option<i32>,
    /// The Monday of the week `today` falls in. Weeks start on Monday, as the
    /// ISO calendar does.
    pub week_start: String,
    /// Net words from `week_start` to `today`.
    pub words_this_week: i32,
    pub weekly_word_goal: Option<i32>,
    /// Consecutive days up to and including today on which they wrote.
    pub streak: i32,
    /// Most recent first, one entry per day the writer showed up.
    pub history: Vec<WritingDay>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGoalsRequest {
    /// `null` clears the target; leaving it out leaves it alone.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<i32>)]
    pub daily_word_goal: Option<Option<i32>>,
    /// The same, for the week.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<i32>)]
    pub weekly_word_goal: Option<Option<i32>>,
}

/// Tells "sent as null" (clear it) apart from "not sent" (leave it): serde
/// alone collapses both into `None`.
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// The Monday on or before a day.
fn monday_of(day: &str) -> Option<String> {
    use chrono::Datelike;
    let date = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let back = date.weekday().num_days_from_monday() as i64;
    Some(
        (date - chrono::Duration::days(back))
            .format("%Y-%m-%d")
            .to_string(),
    )
}

/// Steps back one day. Dates here are already validated `YYYY-MM-DD`.
fn previous_day(day: &str) -> Option<String> {
    let date = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    Some(date.pred_opt()?.format("%Y-%m-%d").to_string())
}

/// Counts back from today while each preceding day has a row.
///
/// Today not being present does not end the streak — the writer may simply not
/// have started yet — so the count begins from yesterday in that case. Showing
/// someone a broken streak at breakfast would be both wrong and discouraging.
fn streak_from(days: &[WritingDay], today: &str) -> i32 {
    use std::collections::HashSet;
    let written: HashSet<&str> = days.iter().map(|d| d.day.as_str()).collect();

    let mut cursor = if written.contains(today) {
        today.to_string()
    } else {
        match previous_day(today) {
            Some(yesterday) if written.contains(yesterday.as_str()) => yesterday,
            _ => return 0,
        }
    };

    let mut streak = 0;
    loop {
        if !written.contains(cursor.as_str()) {
            return streak;
        }
        streak += 1;
        match previous_day(&cursor) {
            Some(previous) => cursor = previous,
            None => return streak,
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/me/writing",
    tag = "goals",
    responses(
        (status = 200, description = "Today, the streak, and recent history", body = WritingSummary),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn writing(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<WritingQuery>,
) -> ApiResult<Json<WritingSummary>> {
    if !valid_day(&query.today) {
        return Err(ApiError::BadRequest(
            "today must look like YYYY-MM-DD".into(),
        ));
    }
    let limit = query.days.unwrap_or(30).clamp(1, 366);

    let summary = blocking(&state.pool, move |conn| {
        let history: Vec<WritingDay> = writing_days::table
            .filter(writing_days::user_id.eq(&user.id))
            .filter(writing_days::day.le(&query.today))
            .order(writing_days::day.desc())
            .limit(limit)
            .select(WritingDay::as_select())
            .load(conn)?;

        let (goal, weekly_goal): (Option<i32>, Option<i32>) = users::table
            .find(&user.id)
            .select((users::daily_word_goal, users::weekly_word_goal))
            .first(conn)?;
        let week_start = monday_of(&query.today).unwrap_or_else(|| query.today.clone());
        // Asked of the table rather than the history list, which may have
        // been asked for fewer days than the week holds.
        let words_this_week: Option<i64> = writing_days::table
            .filter(writing_days::user_id.eq(&user.id))
            .filter(writing_days::day.ge(&week_start))
            .filter(writing_days::day.le(&query.today))
            .select(diesel::dsl::sum(writing_days::net_words))
            .first(conn)?;

        let today_row = history.iter().find(|d| d.day == query.today);
        Ok(WritingSummary {
            words_today: today_row.map(|d| d.net_words).unwrap_or(0),
            saves_today: today_row.map(|d| d.saves).unwrap_or(0),
            daily_word_goal: goal,
            week_start,
            words_this_week: words_this_week.unwrap_or(0) as i32,
            weekly_word_goal: weekly_goal,
            streak: streak_from(&history, &query.today),
            today: query.today,
            history,
        })
    })
    .await?;
    Ok(Json(summary))
}

#[utoipa::path(
    patch,
    path = "/api/me/goals",
    tag = "goals",
    request_body = UpdateGoalsRequest,
    responses(
        (status = 200, description = "The updated profile", body = User),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update_goals(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<UpdateGoalsRequest>,
) -> ApiResult<Json<User>> {
    for goal in [body.daily_word_goal, body.weekly_word_goal]
        .into_iter()
        .flatten()
        .flatten()
    {
        if goal <= 0 {
            return Err(ApiError::BadRequest(
                "a goal has to be a positive number of words".into(),
            ));
        }
    }
    let updated = blocking(&state.pool, move |conn| {
        if let Some(goal) = body.daily_word_goal {
            diesel::update(users::table.find(&user.id))
                .set(users::daily_word_goal.eq(goal))
                .execute(conn)?;
        }
        if let Some(goal) = body.weekly_word_goal {
            diesel::update(users::table.find(&user.id))
                .set(users::weekly_word_goal.eq(goal))
                .execute(conn)?;
        }
        diesel::update(users::table.find(&user.id))
            .set(users::updated_at.eq(now_iso()))
            .execute(conn)?;
        Ok(users::table
            .find(&user.id)
            .select(User::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(updated))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(date: &str) -> WritingDay {
        WritingDay {
            id: date.into(),
            user_id: "u".into(),
            day: date.into(),
            net_words: 100,
            saves: 1,
            created_at: date.into(),
            updated_at: date.into(),
        }
    }

    #[test]
    fn a_week_starts_on_the_monday_on_or_before_the_day() {
        // 2026-10-08 is a Thursday.
        assert_eq!(monday_of("2026-10-08").unwrap(), "2026-10-05");
        assert_eq!(monday_of("2026-10-05").unwrap(), "2026-10-05");
        assert_eq!(
            monday_of("2026-10-11").unwrap(),
            "2026-10-05",
            "Sunday ends the week"
        );
        // Across a month and a year.
        assert_eq!(monday_of("2027-01-01").unwrap(), "2026-12-28");
    }

    #[test]
    fn null_clears_and_absent_leaves_alone() {
        let clear: UpdateGoalsRequest = serde_json::from_str(r#"{"dailyWordGoal":null}"#).unwrap();
        assert_eq!(clear.daily_word_goal, Some(None));
        assert_eq!(clear.weekly_word_goal, None);
        let set: UpdateGoalsRequest = serde_json::from_str(r#"{"weeklyWordGoal":5000}"#).unwrap();
        assert_eq!(set.weekly_word_goal, Some(Some(5000)));
    }

    #[test]
    fn counts_consecutive_days_up_to_today() {
        let days = vec![day("2026-09-22"), day("2026-09-21"), day("2026-09-20")];
        assert_eq!(streak_from(&days, "2026-09-22"), 3);
    }

    #[test]
    fn a_gap_ends_the_streak() {
        let days = vec![day("2026-09-22"), day("2026-09-20"), day("2026-09-19")];
        assert_eq!(streak_from(&days, "2026-09-22"), 1);
    }

    /// Not having written yet today must not read as a broken streak.
    #[test]
    fn today_not_started_still_counts_yesterday() {
        let days = vec![day("2026-09-21"), day("2026-09-20")];
        assert_eq!(streak_from(&days, "2026-09-22"), 2);
    }

    #[test]
    fn two_days_off_is_no_streak() {
        let days = vec![day("2026-09-20")];
        assert_eq!(streak_from(&days, "2026-09-22"), 0);
    }

    #[test]
    fn no_history_is_no_streak() {
        assert_eq!(streak_from(&[], "2026-09-22"), 0);
    }

    #[test]
    fn a_streak_crosses_a_month_boundary() {
        let days = vec![day("2026-09-01"), day("2026-08-31"), day("2026-08-30")];
        assert_eq!(streak_from(&days, "2026-09-01"), 3);
    }

    #[test]
    fn rejects_a_malformed_day() {
        assert!(valid_day("2026-09-22"));
        assert!(!valid_day("2026-9-22"));
        assert!(!valid_day("22/09/2026"));
        assert!(!valid_day(""));
    }
}
