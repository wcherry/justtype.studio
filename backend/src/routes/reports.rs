//! Reporting things that had no way to be reported, and reaching staff.
//!
//! Comments and critiques are reported through their own routes and tables,
//! which keep going to the author as they always have; those routes also call
//! [`reach_staff`], so staff see them. Everything else — Daily Prompts
//! submissions, reader questions and answers, profiles, publications — comes
//! here, into the general `reports` table.
//!
//! `spoiler` and `off_topic` are about craft, so they go only to the author.
//! Every other reason creates or bumps the item in the moderation queue.
use axum::extract::State;
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::clock;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::enums::ReportReason;
use crate::domain::moderation::{self, Flag};
use crate::domain::new_id;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support;

/// Reports one account may make in a day.
const PER_DAY: i64 = 20;
const MAX_DETAIL: usize = 1_000;

/// Whether a reason goes to staff as well as to the author.
pub fn goes_to_staff(reason: &str) -> bool {
    !matches!(reason, "spoiler" | "off_topic")
}

/// Reports `reporter` has made in the last day, on any surface.
fn made_today(conn: &mut Conn, reporter: &str) -> ApiResult<i64> {
    let since = (clock::now() - chrono::Duration::days(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    Ok(reports::table
        .filter(reports::reporter_id.eq(reporter))
        .filter(reports::created_at.gt(&since))
        .count()
        .get_result::<i64>(conn)?
        + comment_reports::table
            .filter(comment_reports::reporter_id.eq(reporter))
            .filter(comment_reports::created_at.gt(&since))
            .count()
            .get_result::<i64>(conn)?
        + prompt_critique_reports::table
            .filter(prompt_critique_reports::reporter_id.eq(reporter))
            .filter(prompt_critique_reports::created_at.gt(&since))
            .count()
            .get_result::<i64>(conn)?)
}

pub fn check_daily_limit(conn: &mut Conn, reporter: &str) -> ApiResult<()> {
    if made_today(conn, reporter)? >= PER_DAY {
        return Err(ApiError::TooManyRequests {
            message: format!("{PER_DAY} reports a day is the most one account can make"),
            retry_after: 24 * 3600,
        });
    }
    Ok(())
}

/// Makes a report from the author-facing routes visible to staff too.
pub fn reach_staff(
    conn: &mut Conn,
    subject_type: &str,
    subject_id: &str,
    author_id: &str,
    reason: &str,
    new_report: bool,
) -> ApiResult<()> {
    if !goes_to_staff(reason) {
        return Ok(());
    }
    moderation::flag(
        conn,
        &Flag {
            subject_type,
            subject_id,
            author_id,
            source: "report",
            reason: Some(reason),
            severity: moderation::severity(reason),
            labels: None,
            hold: false,
            new_report,
        },
    )?;
    Ok(())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReportSubject {
    /// `entry`, `question`, `answer`, `profile` or `publication`.
    pub subject_type: String,
    pub subject_id: String,
    #[schema(value_type = crate::domain::enums::ReportReason)]
    pub reason: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Reported {
    pub reported: bool,
    /// Present for a self-harm report: where to find help.
    pub support: Option<String>,
}

const SUPPORT: &str = "If you or someone you know is thinking about self-harm, please reach out to a crisis line in your country. In the US you can call or text 988.";

#[utoipa::path(
    post,
    path = "/api/reports",
    tag = "moderation",
    request_body = ReportSubject,
    responses(
        (status = 200, description = "Reported", body = Reported),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn report(
    State(state): State<AppState>,
    user: AuthUser,
    Json(body): Json<ReportSubject>,
) -> ApiResult<Json<Reported>> {
    if !ReportReason::accepts(&body.reason) {
        return Err(ApiError::BadRequest(format!(
            "reason must be one of {}",
            ReportReason::options()
        )));
    }
    if !["entry", "question", "answer", "profile", "publication"]
        .contains(&body.subject_type.as_str())
    {
        return Err(ApiError::BadRequest(
            "subjectType must be entry, question, answer, profile or publication".into(),
        ));
    }
    if body
        .detail
        .as_deref()
        .is_some_and(|d| d.chars().count() > MAX_DETAIL)
    {
        return Err(ApiError::BadRequest(format!(
            "keep the detail under {MAX_DETAIL} characters"
        )));
    }
    let self_harm = body.reason == "self_harm";
    blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            let subject = moderation::subject(conn, &body.subject_type, &body.subject_id)?
                .filter(|s| s.public)
                .ok_or_else(|| ApiError::NotFound("nothing to report there".into()))?;
            // You have to be able to see it to report it.
            match body.subject_type.as_str() {
                "publication" => {
                    support::publication_for_reader(conn, &body.subject_id, Some(&user))?;
                }
                "question" | "answer" => {
                    let publication_id: String = if body.subject_type == "question" {
                        review_questions::table
                            .find(&body.subject_id)
                            .select(review_questions::publication_id)
                            .first(conn)?
                    } else {
                        review_answers::table
                            .find(&body.subject_id)
                            .inner_join(review_questions::table)
                            .select(review_questions::publication_id)
                            .first(conn)?
                    };
                    support::publication_for_reader(conn, &publication_id, Some(&user))?;
                }
                _ => {}
            }
            if subject.author_id == user.id {
                return Err(ApiError::BadRequest(
                    "reporting your own work does nothing".into(),
                ));
            }
            check_daily_limit(conn, &user.id)?;
            // The same thing twice is the same report: the reason updates.
            let existing: Option<String> = reports::table
                .filter(reports::subject_type.eq(&body.subject_type))
                .filter(reports::subject_id.eq(&body.subject_id))
                .filter(reports::reporter_id.eq(&user.id))
                .select(reports::id)
                .first(conn)
                .optional()?;
            let new_report = existing.is_none();
            match existing {
                Some(id) => {
                    diesel::update(reports::table.find(&id))
                        .set((
                            reports::reason.eq(&body.reason),
                            reports::detail.eq(body.detail.clone()),
                        ))
                        .execute(conn)?;
                }
                None => {
                    diesel::insert_into(reports::table)
                        .values((
                            reports::id.eq(new_id()),
                            reports::subject_type.eq(&body.subject_type),
                            reports::subject_id.eq(&body.subject_id),
                            reports::subject_author_id.eq(&subject.author_id),
                            reports::reporter_id.eq(&user.id),
                            reports::reason.eq(&body.reason),
                            reports::detail.eq(body.detail.clone()),
                            reports::created_at.eq(clock::now_iso()),
                        ))
                        .execute(conn)?;
                }
            }
            reach_staff(
                conn,
                &body.subject_type,
                &body.subject_id,
                &subject.author_id,
                &body.reason,
                new_report,
            )
        })
    })
    .await?;
    Ok(Json(Reported {
        reported: true,
        support: self_harm.then(|| SUPPORT.to_string()),
    }))
}
