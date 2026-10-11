//! The jobs that run on a schedule, and the loop that runs them.
//!
//! One task, spawned at startup, ticks every minute and runs whatever is due.
//! Daily work is guarded by `job_runs`: a run for a key (`open_prompt` for
//! `2026-10-12`) is claimed before it starts and marked when it ends, so a
//! restart, a missed tick or a second process cannot run it twice, and a run
//! missed while the server was down happens on the first tick back.
//!
//! - `prepare_prompt`, from noon Pacific: gives tomorrow its prompt, so admins
//!   have half a day to see it and change it.
//! - `open_prompt`, from the start of each day: makes sure today has one,
//!   whatever happened to the prepare run.
//! - `build_digests`, from the start of each day: yesterday's inbox summary
//!   for everyone who had any activity.
//! - every tick: the prompt emails that have come due, and delivering the
//!   mail queue. Those keep their own records (`prompt_email_sends`,
//!   `mail_queue`) rather than a run per tick.
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use diesel::prelude::*;
use serde::Serialize;
use serde_json::json;

use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::{new_id, prompts};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub const PREPARE_PROMPT: &str = "prepare_prompt";
pub const OPEN_PROMPT: &str = "open_prompt";
pub const BUILD_DIGESTS: &str = "build_digests";

/// A claim older than this with no finish is a run that died (a crash, a
/// restart mid-job) and may be taken over.
const STALE: i64 = 15;
/// A run that failed is tried again after this long.
const RETRY_AFTER: i64 = 5;

/// When the last tick finished, as Unix seconds, for the status check.
static LAST_TICK: AtomicI64 = AtomicI64::new(0);
/// One tick at a time, whether it came from the loop or the test clock.
static TICKING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn spawn(state: AppState) {
    let seconds: u64 = std::env::var("JUSTTYPE_JOB_TICK_SECONDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|s| *s > 0)
        .unwrap_or(60);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(StdDuration::from_secs(seconds));
        // A tick that ran long is not made up with a burst afterwards.
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            tick(&state).await;
        }
    });
}

/// Runs everything that is due now. A failure in one job is logged and
/// recorded, and the others still run.
pub async fn tick(state: &AppState) {
    let _one_at_a_time = TICKING.lock().await;
    let now = clock::now();
    for (name, result) in [
        (OPEN_PROMPT, open_prompt(state, now).await),
        (PREPARE_PROMPT, prepare_prompt(state, now).await),
        (BUILD_DIGESTS, build_digests(state, now).await),
        (
            "send_prompt_emails",
            send_prompt_emails(state, now).await.map(|_| ()),
        ),
        (
            "deliver_mail",
            crate::mail::deliver_due(&state.pool, &state.mailer)
                .await
                .map(|_| ()),
        ),
    ] {
        if let Err(e) = result {
            tracing::error!(job = name, error = %e, "scheduled job failed");
        }
    }
    LAST_TICK.store(clock::now().timestamp(), Ordering::Relaxed);
}

// --- claiming a run ------------------------------------------------------

fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn parse(t: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(t)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or(DateTime::<Utc>::MIN_UTC)
}

/// Takes the run of `job` for `key`, or says it is not ours to run: already
/// done, still running elsewhere, or failed too recently to try again.
pub fn claim(conn: &mut Conn, job: &str, key: &str, now: DateTime<Utc>) -> ApiResult<bool> {
    let existing: Option<JobRun> = job_runs::table
        .find((job, key))
        .select(JobRun::as_select())
        .first(conn)
        .optional()?;
    let Some(run) = existing else {
        let inserted = diesel::insert_or_ignore_into(job_runs::table)
            .values(&NewJobRun {
                job: job.into(),
                key: key.into(),
                started_at: iso(now),
                finished_at: None,
                error: None,
                attempts: 1,
            })
            .execute(conn)?;
        return Ok(inserted == 1);
    };
    if run.finished_at.is_some() {
        return Ok(false);
    }
    let started = parse(&run.started_at);
    let wait = if run.error.is_some() {
        RETRY_AFTER
    } else {
        STALE
    };
    if now - started < Duration::minutes(wait) {
        return Ok(false);
    }
    // Taken over only if nobody else took it over first.
    let taken = diesel::update(
        job_runs::table
            .filter(job_runs::job.eq(job))
            .filter(job_runs::key.eq(key))
            .filter(job_runs::started_at.eq(&run.started_at))
            .filter(job_runs::finished_at.is_null()),
    )
    .set((
        job_runs::started_at.eq(iso(now)),
        job_runs::error.eq(None::<String>),
        job_runs::attempts.eq(run.attempts + 1),
    ))
    .execute(conn)?;
    Ok(taken == 1)
}

/// Marks a claimed run done, or failed with `error` (to be tried again).
pub fn finish(conn: &mut Conn, job: &str, key: &str, error: Option<String>) -> ApiResult<()> {
    let now = iso(clock::now());
    match error {
        None => diesel::update(job_runs::table.find((job, key)))
            .set((
                job_runs::finished_at.eq(Some(now)),
                job_runs::error.eq(None::<String>),
            ))
            .execute(conn)?,
        Some(e) => diesel::update(job_runs::table.find((job, key)))
            .set((job_runs::error.eq(Some(e)), job_runs::started_at.eq(now)))
            .execute(conn)?,
    };
    Ok(())
}

async fn claimed(
    state: &AppState,
    job: &'static str,
    key: String,
    now: DateTime<Utc>,
) -> ApiResult<bool> {
    blocking(&state.pool, move |conn| claim(conn, job, &key, now)).await
}

async fn finished(
    state: &AppState,
    job: &'static str,
    key: String,
    outcome: &ApiResult<()>,
) -> ApiResult<()> {
    let error = outcome.as_ref().err().map(|e| e.to_string());
    blocking(&state.pool, move |conn| finish(conn, job, &key, error)).await
}

// --- the prompt for a day ------------------------------------------------

/// Noon on `day`, Pacific: when tomorrow's prompt is prepared.
pub fn prepare_time(day: NaiveDate) -> DateTime<Utc> {
    clock::resolve_local(
        clock::prompt_tz(),
        day.and_time(NaiveTime::from_hms_opt(12, 0, 0).expect("noon")),
    )
}

/// Gives `day` a prompt if it has none: the one pinned to it, else the first
/// in the queue, else one the model writes, else (when `evergreen` is set and
/// the model failed or is not configured) one from the built-in list.
pub async fn ensure_prompt(state: &AppState, day: NaiveDate, evergreen: bool) -> ApiResult<()> {
    let day_s = clock::day_string(day);
    let from_queue = {
        let day_s = day_s.clone();
        blocking(&state.pool, move |conn| {
            conn.transaction::<_, ApiError, _>(|conn| {
                let has: i64 = daily_prompts::table
                    .filter(daily_prompts::day.eq(&day_s))
                    .count()
                    .get_result(conn)?;
                if has > 0 {
                    return Ok(true);
                }
                let first: Option<String> = daily_prompts::table
                    .filter(daily_prompts::day.is_null())
                    .order((
                        daily_prompts::position.asc(),
                        daily_prompts::created_at.asc(),
                    ))
                    .select(daily_prompts::id)
                    .first(conn)
                    .optional()?;
                let Some(id) = first else { return Ok(false) };
                diesel::update(daily_prompts::table.find(&id))
                    .set((
                        daily_prompts::day.eq(Some(&day_s)),
                        daily_prompts::position.eq(None::<i32>),
                        daily_prompts::updated_at.eq(clock::now_iso()),
                    ))
                    .execute(conn)?;
                Ok(true)
            })
        })
        .await?
    };
    if from_queue {
        return Ok(());
    }

    // Nothing queued: the model writes one, told the last month's so it does
    // not repeat itself. No writer's text goes with it.
    let written = match &state.config.anthropic_api_key {
        Some(key) => {
            let recent: Vec<String> = blocking(&state.pool, |conn| {
                Ok(daily_prompts::table
                    .filter(daily_prompts::day.is_not_null())
                    .order(daily_prompts::day.desc())
                    .limit(30)
                    .select(daily_prompts::body)
                    .load(conn)?)
            })
            .await?;
            crate::ai::write_prompt(
                key,
                &state.config.anthropic_url,
                &state.config.summary_model,
                &recent,
            )
            .await
            .map(|text| (text, "ai"))
        }
        None => Err(ApiError::internal(
            "no model is configured to write a prompt",
        )),
    };
    let (body, source) = match written {
        Ok(found) => found,
        Err(e) if evergreen => {
            tracing::warn!(error = %e, day = %day_s, "using an evergreen prompt");
            (prompts::evergreen_for(day).to_string(), "evergreen")
        }
        Err(e) => return Err(e),
    };
    blocking(&state.pool, move |conn| {
        let now = clock::now_iso();
        // An admin may have pinned one while the model was writing; theirs wins.
        diesel::insert_or_ignore_into(daily_prompts::table)
            .values(&NewDailyPrompt {
                id: new_id(),
                day: Some(day_s),
                position: None,
                body,
                source: source.into(),
                created_by: None,
                created_at: now.clone(),
                updated_at: now,
            })
            .execute(conn)?;
        Ok(())
    })
    .await
}

async fn prepare_prompt(state: &AppState, now: DateTime<Utc>) -> ApiResult<()> {
    let today = clock::prompt_day(now);
    if now < prepare_time(today) {
        return Ok(());
    }
    let tomorrow = today.succ_opt().expect("a next day");
    let key = clock::day_string(tomorrow);
    if !claimed(state, PREPARE_PROMPT, key.clone(), now).await? {
        return Ok(());
    }
    let outcome = ensure_prompt(state, tomorrow, false).await;
    finished(state, PREPARE_PROMPT, key, &outcome).await?;
    outcome
}

async fn open_prompt(state: &AppState, now: DateTime<Utc>) -> ApiResult<()> {
    let today = clock::prompt_day(now);
    let key = clock::day_string(today);
    if !claimed(state, OPEN_PROMPT, key.clone(), now).await? {
        return Ok(());
    }
    let outcome = ensure_prompt(state, today, true).await;
    finished(state, OPEN_PROMPT, key, &outcome).await?;
    outcome
}

// --- the inbox summary ---------------------------------------------------

/// One writer's day: how many of each kind, and the prompts they were about.
type Activity = (Vec<(String, i64)>, Vec<String>);

/// Builds yesterday's summary for every writer who had activity in it. Safe
/// to repeat: one summary per writer per day, by its key.
pub fn build_digests_for(conn: &mut Conn, yesterday: NaiveDate) -> ApiResult<usize> {
    let from = iso(clock::day_start(yesterday));
    let to = iso(clock::day_end(yesterday));
    let items: Vec<(String, String, Option<String>)> = inbox_items::table
        .filter(inbox_items::created_at.ge(&from))
        .filter(inbox_items::created_at.lt(&to))
        .select((inbox_items::user_id, inbox_items::kind, inbox_items::detail))
        .load(conn)?;
    let mut per_user: std::collections::BTreeMap<String, Activity> = Default::default();
    for (user, kind, detail) in items {
        let (counts, prompts) = per_user.entry(user).or_default();
        match counts.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => *n += 1,
            None => counts.push((kind, 1)),
        }
        let excerpt = detail
            .as_deref()
            .and_then(|d| serde_json::from_str::<serde_json::Value>(d).ok())
            .and_then(|d| {
                d.get("promptExcerpt")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            });
        if let Some(e) = excerpt {
            if !prompts.contains(&e) {
                prompts.push(e);
            }
        }
    }
    let day = clock::day_string(yesterday);
    let mut built = 0;
    for (user, (counts, prompts)) in per_user {
        let summary = json!({
            "counts": counts.iter().map(|(k, n)| json!({ "kind": k, "count": n })).collect::<Vec<_>>(),
            "prompts": prompts,
            "text": crate::domain::inbox::digest_text(&counts, &prompts),
        });
        built += diesel::insert_or_ignore_into(inbox_digests::table)
            .values((
                inbox_digests::user_id.eq(&user),
                inbox_digests::day.eq(&day),
                inbox_digests::summary_json.eq(summary.to_string()),
                inbox_digests::created_at.eq(clock::now_iso()),
            ))
            .execute(conn)?;
    }
    Ok(built)
}

async fn build_digests(state: &AppState, now: DateTime<Utc>) -> ApiResult<()> {
    let today = clock::prompt_day(now);
    let key = clock::day_string(today);
    if !claimed(state, BUILD_DIGESTS, key.clone(), now).await? {
        return Ok(());
    }
    let yesterday = today.pred_opt().expect("a day before");
    let outcome = blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| build_digests_for(conn, yesterday).map(|_| ()))
    })
    .await;
    finished(state, BUILD_DIGESTS, key, &outcome).await?;
    outcome
}

// --- the prompt email ----------------------------------------------------

/// Queues today's prompt for every writer whose time has come and who has not
/// had it. Returns how many were queued.
pub async fn send_prompt_emails(state: &AppState, now: DateTime<Utc>) -> ApiResult<usize> {
    let public_url = state.config.public_url.clone();
    blocking(&state.pool, move |conn| {
        let today = clock::prompt_day(now);
        let day = clock::day_string(today);
        let Some(prompt): Option<DailyPrompt> = daily_prompts::table
            .filter(daily_prompts::day.eq(&day))
            .select(DailyPrompt::as_select())
            .first(conn)
            .optional()?
        else {
            return Ok(0);
        };
        let already = prompt_email_sends::table
            .filter(prompt_email_sends::day.eq(&day))
            .select(prompt_email_sends::user_id);
        let waiting: Vec<(PromptEnrollment, String, String)> = prompt_enrollments::table
            .inner_join(users::table)
            .filter(prompt_enrollments::email_enabled.eq(true))
            .filter(prompt_enrollments::left_at.is_null())
            .filter(users::email_verified_at.is_not_null())
            .filter(prompt_enrollments::user_id.ne_all(already))
            .select((
                PromptEnrollment::as_select(),
                users::email,
                users::display_name,
            ))
            .load(conn)?;
        let mut queued = 0;
        for (enrollment, email, name) in waiting {
            let Ok(tz) = enrollment.email_timezone.parse::<chrono_tz::Tz>() else {
                continue;
            };
            let Some(time) = clock::parse_time(&enrollment.email_local_time) else {
                continue;
            };
            if now < prompts::send_instant(today, time, tz) {
                continue;
            }
            let message = prompt_message(
                &public_url,
                &email,
                &name,
                &day,
                &prompt.body,
                &enrollment.unsubscribe_token,
            );
            conn.transaction::<_, ApiError, _>(|conn| {
                let recorded = diesel::insert_or_ignore_into(prompt_email_sends::table)
                    .values((
                        prompt_email_sends::user_id.eq(&enrollment.user_id),
                        prompt_email_sends::day.eq(&day),
                        prompt_email_sends::sent_at.eq(iso(now)),
                    ))
                    .execute(conn)?;
                if recorded == 1 {
                    crate::mail::enqueue(conn, &message, clock::day_end(today))?;
                    queued += 1;
                }
                Ok(())
            })?;
        }
        Ok(queued)
    })
    .await
}

pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The prompt email: the prompt, a link to write, a link to read, and a
/// one-click way out.
pub fn prompt_message(
    public_url: &str,
    to: &str,
    name: &str,
    day: &str,
    prompt: &str,
    token: &str,
) -> crate::mail::Message {
    let write = format!("{public_url}/prompts/{day}/write");
    let read = format!("{public_url}/prompts/{day}");
    let leave = format!("{public_url}/prompts/unsubscribe?token={token}");
    let one_click = format!("{public_url}/api/prompts/unsubscribe?token={token}");
    let body = format!(
        "Hello {name},\n\nToday's prompt:\n\n{prompt}\n\nWrite your answer:\n{write}\n\nRead what others made of it:\n{read}\n\n--\nYou get this because you turned on the Daily Prompts email. Stop it with one click:\n{leave}\n"
    );
    let paragraphs: String = prompt
        .split("\n\n")
        .map(|p| {
            format!(
                "<p style=\"font-size:18px;line-height:1.5\">{}</p>",
                escape(p).replace('\n', "<br>")
            )
        })
        .collect();
    let html = format!(
        "<!doctype html><html><body style=\"font-family:Georgia,serif;color:#222;max-width:560px;margin:0 auto;padding:24px\">\
<p>Hello {name},</p><p>Today's prompt:</p>{paragraphs}\
<p><a href=\"{write}\" style=\"display:inline-block;padding:10px 16px;background:#7a4f2a;color:#fff;text-decoration:none;border-radius:4px\">Write your answer</a></p>\
<p><a href=\"{read}\">Read what others made of it</a></p>\
<hr style=\"border:none;border-top:1px solid #ddd\"><p style=\"font-size:12px;color:#777\">You get this because you turned on the Daily Prompts email. \
<a href=\"{leave}\">Stop these emails</a>.</p></body></html>",
        name = escape(name),
        write = escape(&write),
        read = escape(&read),
        leave = escape(&leave),
    );
    crate::mail::Message {
        to: to.into(),
        subject: format!(
            "Today's prompt: {}",
            crate::domain::split::shorten(prompt, 60)
        ),
        body,
        html: Some(html),
        headers: vec![
            ("List-Unsubscribe".into(), format!("<{one_click}>")),
            (
                "List-Unsubscribe-Post".into(),
                "List-Unsubscribe=One-Click".into(),
            ),
        ],
    }
}

// --- whether it is all running -------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobHealth {
    pub ok: bool,
    /// What is late, if anything.
    pub problems: Vec<String>,
    pub last_tick: Option<String>,
}

/// What should have run by now and has not.
pub fn health(conn: &mut Conn, now: DateTime<Utc>, started: DateTime<Utc>) -> ApiResult<JobHealth> {
    let mut problems = Vec::new();
    let grace = Duration::minutes(10);
    let last = LAST_TICK.load(Ordering::Relaxed);
    let last_tick = (last > 0)
        .then(|| DateTime::<Utc>::from_timestamp(last, 0))
        .flatten();
    match last_tick {
        Some(t) if now - t > Duration::minutes(5) => problems.push(format!(
            "the job runner last ticked {} minutes ago",
            (now - t).num_minutes()
        )),
        None if now - started > Duration::minutes(5) => {
            problems.push("the job runner has not ticked since the server started".into())
        }
        _ => {}
    }
    let today = clock::prompt_day(now);
    let mut expect = |job: &str, key: String, due: DateTime<Utc>| -> ApiResult<()> {
        if now < due + grace {
            return Ok(());
        }
        let done: Option<Option<String>> = job_runs::table
            .find((job, &key))
            .select(job_runs::finished_at)
            .first(conn)
            .optional()?;
        if !matches!(done, Some(Some(_))) {
            problems.push(format!("{job} for {key} has not run"));
        }
        Ok(())
    };
    expect(
        OPEN_PROMPT,
        clock::day_string(today),
        clock::day_start(today),
    )?;
    expect(
        BUILD_DIGESTS,
        clock::day_string(today),
        clock::day_start(today),
    )?;
    expect(
        PREPARE_PROMPT,
        clock::day_string(today.succ_opt().expect("a next day")),
        prepare_time(today),
    )?;
    Ok(JobHealth {
        ok: problems.is_empty(),
        problems,
        last_tick: last_tick.map(iso),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use diesel::connection::SimpleConnection;
    use diesel_migrations::MigrationHarness;

    pub fn conn() -> Conn {
        let pool = crate::db::build_pool_of(":memory:", 1).unwrap();
        let mut c = pool.get().unwrap();
        c.batch_execute("PRAGMA foreign_keys = ON;").unwrap();
        c.run_pending_migrations(crate::db::MIGRATIONS).unwrap();
        c
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn a_finished_run_is_not_run_again() {
        let mut c = conn();
        let now = utc("2026-10-12T08:00:00Z");
        assert!(claim(&mut c, OPEN_PROMPT, "2026-10-12", now).unwrap());
        finish(&mut c, OPEN_PROMPT, "2026-10-12", None).unwrap();
        // A restart later in the day: nothing to do.
        assert!(!claim(&mut c, OPEN_PROMPT, "2026-10-12", now + Duration::hours(5)).unwrap());
        // The next day is its own run.
        assert!(claim(&mut c, OPEN_PROMPT, "2026-10-13", now + Duration::hours(24)).unwrap());
    }

    #[test]
    fn a_run_still_going_is_not_started_twice() {
        let mut c = conn();
        let now = utc("2026-10-12T08:00:00Z");
        assert!(claim(&mut c, PREPARE_PROMPT, "2026-10-13", now).unwrap());
        // The next tick, a minute later: still running, leave it.
        assert!(!claim(
            &mut c,
            PREPARE_PROMPT,
            "2026-10-13",
            now + Duration::minutes(1)
        )
        .unwrap());
        // Long after, with no finish: it died, and is taken over once.
        let later = now + Duration::minutes(STALE + 1);
        assert!(claim(&mut c, PREPARE_PROMPT, "2026-10-13", later).unwrap());
        assert!(!claim(&mut c, PREPARE_PROMPT, "2026-10-13", later).unwrap());
    }

    #[test]
    fn a_failed_run_is_tried_again_after_a_while() {
        let mut c = conn();
        let now = clock::now();
        assert!(claim(&mut c, PREPARE_PROMPT, "k", now).unwrap());
        finish(
            &mut c,
            PREPARE_PROMPT,
            "k",
            Some("the model was busy".into()),
        )
        .unwrap();
        assert!(!claim(&mut c, PREPARE_PROMPT, "k", now + Duration::minutes(1)).unwrap());
        assert!(claim(
            &mut c,
            PREPARE_PROMPT,
            "k",
            now + Duration::minutes(RETRY_AFTER + 1)
        )
        .unwrap());
    }

    #[test]
    fn a_job_missed_while_down_counts_as_late() {
        let mut c = conn();
        // Just past midnight plus the grace, and today's open has not run.
        let today = clock::parse_day("2026-10-12").unwrap();
        let now = clock::day_start(today) + Duration::minutes(30);
        let report = health(&mut c, now, now).unwrap();
        assert!(!report.ok);
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.contains("open_prompt for 2026-10-12")),
            "{:?}",
            report.problems
        );
        claim(&mut c, OPEN_PROMPT, "2026-10-12", now).unwrap();
        finish(&mut c, OPEN_PROMPT, "2026-10-12", None).unwrap();
        claim(&mut c, BUILD_DIGESTS, "2026-10-12", now).unwrap();
        finish(&mut c, BUILD_DIGESTS, "2026-10-12", None).unwrap();
        let report = health(&mut c, now, now).unwrap();
        assert!(
            !report
                .problems
                .iter()
                .any(|p| p.contains("2026-10-12 has not run")),
            "{:?}",
            report.problems
        );
    }

    #[test]
    fn the_prompt_email_has_both_parts_and_a_way_out() {
        let m = prompt_message(
            "https://j.test",
            "a@b.c",
            "Ada <3",
            "2026-10-12",
            "A door.\n\nIt opens.",
            "tok",
        );
        assert!(m.body.contains("https://j.test/prompts/2026-10-12/write"));
        assert!(m
            .body
            .contains("https://j.test/prompts/unsubscribe?token=tok"));
        let html = m.html.unwrap();
        assert!(
            html.contains("Ada &lt;3")
                && html.contains("<p style=\"font-size:18px;line-height:1.5\">It opens.</p>")
        );
        assert!(m.headers.iter().any(|(k, v)| k == "List-Unsubscribe"
            && v == "<https://j.test/api/prompts/unsubscribe?token=tok>"));
        assert!(m
            .headers
            .iter()
            .any(|(k, v)| k == "List-Unsubscribe-Post" && v == "List-Unsubscribe=One-Click"));
    }
}
