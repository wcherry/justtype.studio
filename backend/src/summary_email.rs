//! The summary email: what the writer wrote, what is still on their to-do
//! lists, and what reviewers said, every morning or every Monday morning.
//!
//! It goes at seven in the morning in the writer's own timezone, the one their
//! device sent when they chose it, because the words they write are counted
//! against their local day (see `routes::goals`). A daily summary covers the
//! day before; a weekly one, sent on Monday, covers Monday to Sunday before it.
//!
//! Each period is recorded in `summary_email_sends` in the same transaction
//! that queues its message, so it goes once. A period with nothing in it — no
//! words, no comments, nothing ticked off — is recorded and not sent: an email
//! that only repeats the to-do list is one people learn to ignore. Only the
//! latest period is ever due, so a server that was down for a week sends one
//! summary when it comes back, not seven.
use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;
use diesel::prelude::*;

use crate::clock;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::enums::SummaryEmail;
use crate::error::{ApiError, ApiResult};
use crate::jobs::escape;
use crate::state::AppState;

/// The local hour the summary goes out.
pub const SEND_HOUR: u32 = 7;
/// How many open tasks, and how many comments, are shown for each book; the
/// rest are counted.
const SHOWN: usize = 5;

/// The days a summary covers, both ends included, in the writer's timezone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    pub often: SummaryEmail,
    pub first: NaiveDate,
    pub last: NaiveDate,
}

impl Period {
    /// `daily:2026-10-21`, `weekly:2026-10-12`: one row per writer in
    /// `summary_email_sends`.
    pub fn key(&self) -> String {
        format!("{}:{}", self.often.as_str(), clock::day_string(self.first))
    }

    fn days(&self) -> impl Iterator<Item = NaiveDate> {
        self.first.iter_days().take_while({
            let last = self.last;
            move |d| *d <= last
        })
    }
}

/// The latest period whose summary is due by `now` for a writer in `tz`, and
/// when it fell due. `None` when the summary is off, or this week's Monday
/// morning has not come yet.
pub fn due(often: SummaryEmail, tz: Tz, now: DateTime<Utc>) -> Option<(Period, DateTime<Utc>)> {
    let today = clock::prompt_day_in(tz, now);
    let send_day = match often {
        SummaryEmail::None => return None,
        SummaryEmail::Daily => today,
        SummaryEmail::Weekly => {
            today - Duration::days(today.weekday().num_days_from_monday() as i64)
        }
    };
    let at = clock::resolve_local(
        tz,
        send_day.and_time(NaiveTime::from_hms_opt(SEND_HOUR, 0, 0).expect("an hour")),
    );
    if now < at {
        return None;
    }
    let last = send_day.pred_opt()?;
    let first = match often {
        SummaryEmail::Weekly => send_day - Duration::days(7),
        _ => last,
    };
    Some((Period { often, first, last }, at))
}

// --- what goes in it -----------------------------------------------------

#[derive(Debug, Default)]
pub struct Summary {
    /// Net words for each day of the period that had a save.
    pub days: BTreeMap<NaiveDate, i32>,
    pub daily_goal: Option<i32>,
    pub weekly_goal: Option<i32>,
    /// One per book with open tasks or tasks done in the period.
    pub tasks: Vec<BookTasks>,
    /// One per book with new comments from readers in the period.
    pub comments: Vec<BookComments>,
}

#[derive(Debug)]
pub struct BookTasks {
    pub work_id: String,
    pub title: String,
    pub open: Vec<String>,
    pub open_count: usize,
    pub done: usize,
}

#[derive(Debug)]
pub struct BookComments {
    pub title: String,
    pub count: usize,
    /// Who said it, an excerpt, and the review it was left on.
    pub shown: Vec<(String, String, String)>,
}

impl Summary {
    pub fn words(&self) -> i64 {
        self.days.values().map(|n| *n as i64).sum()
    }

    fn comment_count(&self) -> usize {
        self.comments.iter().map(|c| c.count).sum()
    }

    fn done_count(&self) -> usize {
        self.tasks.iter().map(|t| t.done).sum()
    }

    /// Whether anything happened: worth an email.
    pub fn is_worth_sending(&self) -> bool {
        !self.days.is_empty() || self.comment_count() > 0 || self.done_count() > 0
    }
}

/// The books whose to-do lists and reviews are the writer's business: their
/// own, and those they were brought in on to write or edit.
fn books(conn: &mut Conn, user_id: &str) -> ApiResult<Vec<(String, String)>> {
    let shared = work_collaborators::table
        .filter(work_collaborators::user_id.eq(user_id))
        .filter(work_collaborators::accepted_at.is_not_null())
        .filter(work_collaborators::role.eq_any(["owner", "writer", "editor"]))
        .select(work_collaborators::work_id);
    Ok(works::table
        .filter(works::owner_id.eq(user_id).or(works::id.eq_any(shared)))
        .order((works::title.asc(), works::id.asc()))
        .select((works::id, works::title))
        .load(conn)?)
}

fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Everything the summary for `period` says.
pub fn gather(conn: &mut Conn, user_id: &str, tz: Tz, period: Period) -> ApiResult<Summary> {
    let mut summary = Summary::default();
    let (daily_goal, weekly_goal): (Option<i32>, Option<i32>) = users::table
        .find(user_id)
        .select((users::daily_word_goal, users::weekly_word_goal))
        .first(conn)?;
    summary.daily_goal = daily_goal;
    summary.weekly_goal = weekly_goal;

    let days: Vec<(String, i32)> = writing_days::table
        .filter(writing_days::user_id.eq(user_id))
        .filter(writing_days::day.ge(clock::day_string(period.first)))
        .filter(writing_days::day.le(clock::day_string(period.last)))
        .select((writing_days::day, writing_days::net_words))
        .load(conn)?;
    summary.days = days
        .into_iter()
        .filter_map(|(d, n)| Some((clock::parse_day(&d)?, n)))
        .collect();

    let from = iso(clock::day_start_in(tz, period.first));
    let to = iso(clock::day_start_in(
        tz,
        period.last.succ_opt().unwrap_or(period.last),
    ));
    let books = books(conn, user_id)?;
    let ids: Vec<&String> = books.iter().map(|(id, _)| id).collect();

    let tasks: Vec<(String, String, String, Option<String>)> = revision_tasks::table
        .filter(revision_tasks::work_id.eq_any(&ids))
        .filter(
            revision_tasks::state.eq("open").or(revision_tasks::done_at
                .ge(&from)
                .and(revision_tasks::done_at.lt(&to))),
        )
        .order(revision_tasks::created_at.asc())
        .select((
            revision_tasks::work_id,
            revision_tasks::body,
            revision_tasks::state,
            revision_tasks::done_at,
        ))
        .load(conn)?;

    let comments: Vec<(String, String, String, String)> = comments::table
        .inner_join(publications::table)
        .inner_join(users::table.on(users::id.eq(comments::author_id)))
        .filter(publications::work_id.eq_any(&ids))
        .filter(comments::author_id.ne(user_id))
        .filter(comments::moderation_state.eq("visible"))
        .filter(comments::state.ne("deleted"))
        .filter(comments::created_at.ge(&from))
        .filter(comments::created_at.lt(&to))
        .order(comments::created_at.asc())
        .select((
            publications::work_id,
            comments::publication_id,
            users::display_name,
            comments::body,
        ))
        .load(conn)?;

    for (work_id, title) in &books {
        let mine: Vec<_> = tasks.iter().filter(|t| &t.0 == work_id).collect();
        let open: Vec<&String> = mine
            .iter()
            .filter(|t| t.2 == "open")
            .map(|t| &t.1)
            .collect();
        let done = mine.iter().filter(|t| t.2 == "done").count();
        if !open.is_empty() || done > 0 {
            summary.tasks.push(BookTasks {
                work_id: work_id.clone(),
                title: title.clone(),
                open_count: open.len(),
                open: open.iter().take(SHOWN).map(|b| one_line(b, 120)).collect(),
                done,
            });
        }
        let said: Vec<_> = comments.iter().filter(|c| &c.0 == work_id).collect();
        if !said.is_empty() {
            summary.comments.push(BookComments {
                title: title.clone(),
                count: said.len(),
                shown: said
                    .iter()
                    .take(SHOWN)
                    .map(|(_, publication, who, body)| {
                        (who.clone(), one_line(body, 160), publication.clone())
                    })
                    .collect(),
            });
        }
    }
    Ok(summary)
}

/// Whitespace folded, and cut to `max` characters.
fn one_line(text: &str, max: usize) -> String {
    let folded = text.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::domain::split::shorten(&folded, max)
}

// --- how it reads ----------------------------------------------------------

/// `12,345`.
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!(
        "{} {}",
        thousands(n as i64),
        if n == 1 { one } else { many }
    )
}

/// `Tuesday 21 October`, or `13–19 October`, or `29 September – 5 October`.
pub fn describe(period: Period) -> String {
    if period.first == period.last {
        return period.first.format("%A %-d %B").to_string();
    }
    if period.first.month() == period.last.month() {
        format!(
            "{}–{}",
            period.first.format("%-d"),
            period.last.format("%-d %B")
        )
    } else {
        format!(
            "{} – {}",
            period.first.format("%-d %B"),
            period.last.format("%-d %B")
        )
    }
}

/// The paragraph about the words.
fn writing_lines(summary: &Summary, period: Period) -> Vec<String> {
    let words = summary.words();
    let mut lines = vec![match (summary.days.is_empty(), words) {
        (true, _) => "You didn't write in this stretch. The page will keep.".to_string(),
        (false, n) if n > 0 => format!("You wrote {} words.", thousands(n)),
        (false, n) if n < 0 => format!("You cut {} words.", thousands(-n)),
        _ => "You revised, and the length came out even.".to_string(),
    }];
    if period.often == SummaryEmail::Weekly {
        let written = summary.days.len();
        if written > 0 {
            lines.push(format!(
                "You wrote on {} of 7.",
                plural(written, "day", "days")
            ));
        }
        let by_day: Vec<String> = period
            .days()
            .map(|d| {
                let n = summary.days.get(&d).copied();
                format!(
                    "{} {}",
                    d.format("%a"),
                    n.map(|n| thousands(n as i64)).unwrap_or_else(|| "–".into())
                )
            })
            .collect();
        lines.push(by_day.join(" · "));
        if let Some(goal) = summary.weekly_goal.filter(|g| *g > 0) {
            lines.push(if words >= goal as i64 {
                format!("That meets your weekly goal of {}.", thousands(goal as i64))
            } else {
                format!(
                    "Your weekly goal is {}: {} to go next time.",
                    thousands(goal as i64),
                    thousands(goal as i64 - words)
                )
            });
        }
        if let Some(goal) = summary.daily_goal.filter(|g| *g > 0) {
            let met = summary.days.values().filter(|n| **n >= goal).count();
            lines.push(format!(
                "You met your daily goal of {} on {}.",
                thousands(goal as i64),
                plural(met, "day", "days")
            ));
        }
    } else if let Some(goal) = summary.daily_goal.filter(|g| *g > 0) {
        if !summary.days.is_empty() {
            lines.push(if words >= goal as i64 {
                format!("That meets your daily goal of {}.", thousands(goal as i64))
            } else {
                format!("Your daily goal is {}.", thousands(goal as i64))
            });
        }
    }
    lines
}

/// The subject line: the period, and the numbers that matter.
fn subject(summary: &Summary, period: Period) -> String {
    let lead = match period.often {
        SummaryEmail::Weekly => "Your week",
        _ => "Your day",
    };
    let mut parts = Vec::new();
    let words = summary.words();
    if words > 0 {
        parts.push(format!("{} words", thousands(words)));
    }
    let comments = summary.comment_count();
    if comments > 0 {
        parts.push(plural(comments, "new comment", "new comments"));
    }
    let done = summary.done_count();
    if done > 0 {
        parts.push(plural(done, "task done", "tasks done"));
    }
    if parts.is_empty() {
        format!("{lead} in writing")
    } else {
        format!("{lead} in writing: {}", parts.join(", "))
    }
}

/// The email itself: both parts, and a one-click way out.
pub fn message(
    public_url: &str,
    to: &str,
    name: &str,
    period: Period,
    summary: &Summary,
    token: &str,
) -> crate::mail::Message {
    let leave = format!("{public_url}/summary-email/unsubscribe?token={token}");
    let one_click = format!("{public_url}/api/summary-email/unsubscribe?token={token}");
    let settings = format!("{public_url}/settings/general#summary-email");
    let when = describe(period);
    let writing = writing_lines(summary, period);
    let why = match period.often {
        SummaryEmail::Weekly => "a weekly",
        _ => "a daily",
    };

    // Plain text.
    let mut body = format!("Hello {name},\n\nHere is {when}.\n\nWRITING\n\n");
    for line in &writing {
        body.push_str(line);
        body.push('\n');
    }
    if !summary.comments.is_empty() {
        body.push_str("\nREVIEW COMMENTS\n");
        for book in &summary.comments {
            body.push_str(&format!(
                "\n{} — {}\n",
                book.title,
                plural(book.count, "new comment", "new comments")
            ));
            for (who, said, publication) in &book.shown {
                body.push_str(&format!(
                    "  {who}: \"{said}\"\n  {public_url}/review/{publication}\n"
                ));
            }
            if book.count > book.shown.len() {
                body.push_str(&format!("  and {} more\n", book.count - book.shown.len()));
            }
        }
    }
    if !summary.tasks.is_empty() {
        body.push_str("\nTO DO\n");
        for book in &summary.tasks {
            let mut head = format!(
                "\n{} — {} open",
                book.title,
                thousands(book.open_count as i64)
            );
            if book.done > 0 {
                head.push_str(&format!(", {} done", thousands(book.done as i64)));
            }
            body.push_str(&format!("{head}\n"));
            for task in &book.open {
                body.push_str(&format!("  [ ] {task}\n"));
            }
            if book.open_count > book.open.len() {
                body.push_str(&format!(
                    "  and {} more\n",
                    book.open_count - book.open.len()
                ));
            }
            body.push_str(&format!("  {public_url}/works/{}\n", book.work_id));
        }
    }
    body.push_str(&format!(
        "\n--\nYou get this because you asked for {why} summary. Change how often in your settings:\n{settings}\nOr stop it with one click:\n{leave}\n"
    ));

    // HTML.
    let h2 = "font-size:13px;letter-spacing:.08em;text-transform:uppercase;color:#7a4f2a;margin:28px 0 8px";
    let h3 = "font-size:16px;margin:16px 0 6px";
    let muted = "color:#777";
    let mut html = format!(
        "<!doctype html><html><body style=\"font-family:Georgia,serif;color:#222;max-width:560px;margin:0 auto;padding:24px;line-height:1.5\">\
<p>Hello {},</p><p>Here is {}.</p><h2 style=\"{h2}\">Writing</h2>",
        escape(name),
        escape(&when)
    );
    for (i, line) in writing.iter().enumerate() {
        let style = if i == 0 { "font-size:18px" } else { muted };
        html.push_str(&format!(
            "<p style=\"{style};margin:4px 0\">{}</p>",
            escape(line)
        ));
    }
    if !summary.comments.is_empty() {
        html.push_str(&format!("<h2 style=\"{h2}\">Review comments</h2>"));
        for book in &summary.comments {
            html.push_str(&format!(
                "<h3 style=\"{h3}\">{} <span style=\"{muted};font-weight:normal\">· {}</span></h3>",
                escape(&book.title),
                escape(&plural(book.count, "new comment", "new comments"))
            ));
            for (who, said, publication) in &book.shown {
                html.push_str(&format!(
                    "<p style=\"margin:6px 0\"><b>{}</b>: “{}” <a href=\"{}\">Read</a></p>",
                    escape(who),
                    escape(said),
                    escape(&format!("{public_url}/review/{publication}"))
                ));
            }
            if book.count > book.shown.len() {
                html.push_str(&format!(
                    "<p style=\"{muted};margin:6px 0\">and {} more</p>",
                    book.count - book.shown.len()
                ));
            }
        }
    }
    if !summary.tasks.is_empty() {
        html.push_str(&format!("<h2 style=\"{h2}\">To do</h2>"));
        for book in &summary.tasks {
            let mut counts = format!("{} open", thousands(book.open_count as i64));
            if book.done > 0 {
                counts.push_str(&format!(", {} done", thousands(book.done as i64)));
            }
            html.push_str(&format!(
                "<h3 style=\"{h3}\"><a href=\"{}\" style=\"color:#222\">{}</a> <span style=\"{muted};font-weight:normal\">· {}</span></h3>",
                escape(&format!("{public_url}/works/{}", book.work_id)),
                escape(&book.title),
                escape(&counts)
            ));
            if !book.open.is_empty() {
                html.push_str("<ul style=\"margin:4px 0;padding-left:20px\">");
                for task in &book.open {
                    html.push_str(&format!("<li>{}</li>", escape(task)));
                }
                html.push_str("</ul>");
            }
            if book.open_count > book.open.len() {
                html.push_str(&format!(
                    "<p style=\"{muted};margin:4px 0\">and {} more</p>",
                    book.open_count - book.open.len()
                ));
            }
        }
    }
    html.push_str(&format!(
        "<hr style=\"border:none;border-top:1px solid #ddd;margin-top:32px\"><p style=\"font-size:12px;{muted}\">You get this because you asked for {why} summary. \
<a href=\"{}\">Change how often</a> or <a href=\"{}\">stop these emails</a>.</p></body></html>",
        escape(&settings),
        escape(&leave)
    ));

    crate::mail::Message {
        to: to.into(),
        subject: subject(summary, period),
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

// --- sending ---------------------------------------------------------------

/// Queues the summary for every writer whose summary has come due and not been
/// handled. Returns how many were queued.
pub async fn send_due(state: &AppState, now: DateTime<Utc>) -> ApiResult<usize> {
    let public_url = state.config.public_url.clone();
    blocking(&state.pool, move |conn| {
        type Row = (String, String, String, String, String, String, String);
        let waiting: Vec<Row> = summary_email_settings::table
            .inner_join(users::table)
            .filter(summary_email_settings::often.ne(SummaryEmail::None.as_str()))
            .filter(users::email_verified_at.is_not_null())
            .filter(users::deactivated_at.is_null())
            .filter(users::closing_at.is_null())
            .select((
                users::id,
                users::email,
                users::display_name,
                summary_email_settings::often,
                summary_email_settings::timezone,
                summary_email_settings::updated_at,
                summary_email_settings::token,
            ))
            .load(conn)?;
        let mut queued = 0;
        for (user_id, email, name, often, tz, since, token) in waiting {
            let often = match often.as_str() {
                "daily" => SummaryEmail::Daily,
                "weekly" => SummaryEmail::Weekly,
                _ => continue,
            };
            let tz = tz.parse::<Tz>().unwrap_or_else(|_| clock::prompt_tz());
            let Some((period, at)) = due(often, tz, now) else {
                continue;
            };
            // Chosen after it fell due: the first one is the next one.
            if since > iso(at) {
                continue;
            }
            let key = period.key();
            let handled: i64 = summary_email_sends::table
                .filter(summary_email_sends::user_id.eq(&user_id))
                .filter(summary_email_sends::period.eq(&key))
                .count()
                .get_result(conn)?;
            if handled > 0 {
                continue;
            }
            let summary = gather(conn, &user_id, tz, period)?;
            let worth = summary.is_worth_sending();
            conn.transaction::<_, ApiError, _>(|conn| {
                let recorded = diesel::insert_or_ignore_into(summary_email_sends::table)
                    .values((
                        summary_email_sends::user_id.eq(&user_id),
                        summary_email_sends::period.eq(&key),
                        summary_email_sends::sent_at.eq(iso(now)),
                        summary_email_sends::queued.eq(worth),
                    ))
                    .execute(conn)?;
                if recorded == 1 && worth {
                    let message = message(&public_url, &email, &name, period, &summary, &token);
                    // Stale by the next morning's.
                    crate::mail::enqueue(conn, &message, at + Duration::days(1))?;
                    queued += 1;
                }
                Ok(())
            })?;
        }
        Ok(queued)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::{America::New_York, Europe::London};

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn day(s: &str) -> NaiveDate {
        clock::parse_day(s).unwrap()
    }

    #[test]
    fn daily_is_yesterday_from_seven_local() {
        // 06:59 in London (BST, UTC+1) on Wednesday: not yet.
        assert_eq!(
            due(SummaryEmail::Daily, London, utc("2026-10-21T05:59:00Z")),
            None
        );
        let (period, at) = due(SummaryEmail::Daily, London, utc("2026-10-21T06:00:00Z")).unwrap();
        assert_eq!(at, utc("2026-10-21T06:00:00Z"));
        assert_eq!(
            (period.first, period.last),
            (day("2026-10-20"), day("2026-10-20"))
        );
        assert_eq!(period.key(), "daily:2026-10-20");
        // Late that night it is still the same one.
        let (late, _) = due(SummaryEmail::Daily, London, utc("2026-10-21T22:00:00Z")).unwrap();
        assert_eq!(late, period);
        assert_eq!(
            due(SummaryEmail::None, London, utc("2026-10-21T22:00:00Z")),
            None
        );
    }

    #[test]
    fn weekly_is_the_week_before_from_monday_morning() {
        // Monday 19 October, 07:00 in New York (EDT, UTC−4).
        assert_eq!(
            due(SummaryEmail::Weekly, New_York, utc("2026-10-19T10:59:00Z")),
            None
        );
        let (period, at) =
            due(SummaryEmail::Weekly, New_York, utc("2026-10-19T11:00:00Z")).unwrap();
        assert_eq!(at, utc("2026-10-19T11:00:00Z"));
        assert_eq!(
            (period.first, period.last),
            (day("2026-10-12"), day("2026-10-18"))
        );
        assert_eq!(period.key(), "weekly:2026-10-12");
        // Thursday: still last Monday's.
        let (thursday, _) =
            due(SummaryEmail::Weekly, New_York, utc("2026-10-22T15:00:00Z")).unwrap();
        assert_eq!(thursday, period);
        // Before seven the Monday after, nothing is: last week's has been
        // handled, and the week just gone is not due yet.
        assert_eq!(
            due(SummaryEmail::Weekly, New_York, utc("2026-10-26T10:00:00Z")),
            None
        );
    }

    #[test]
    fn numbers_and_dates_read_naturally() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_240), "1,240");
        assert_eq!(thousands(-1_234_567), "-1,234,567");
        let p = |f: &str, l: &str, often| Period {
            often,
            first: day(f),
            last: day(l),
        };
        assert_eq!(
            describe(p("2026-10-20", "2026-10-20", SummaryEmail::Daily)),
            "Tuesday 20 October"
        );
        assert_eq!(
            describe(p("2026-10-12", "2026-10-18", SummaryEmail::Weekly)),
            "12–18 October"
        );
        assert_eq!(
            describe(p("2026-09-28", "2026-10-04", SummaryEmail::Weekly)),
            "28 September – 4 October"
        );
    }

    #[test]
    fn the_email_says_what_happened_and_escapes_it() {
        let period = Period {
            often: SummaryEmail::Weekly,
            first: day("2026-10-12"),
            last: day("2026-10-18"),
        };
        let summary = Summary {
            days: [(day("2026-10-12"), 900), (day("2026-10-14"), 1_500)]
                .into_iter()
                .collect(),
            daily_goal: Some(1_000),
            weekly_goal: Some(5_000),
            tasks: vec![BookTasks {
                work_id: "w1".into(),
                title: "Tide <draft>".into(),
                open: vec!["Make chapter four colder".into()],
                open_count: 1,
                done: 2,
            }],
            comments: vec![BookComments {
                title: "Tide <draft>".into(),
                count: 7,
                shown: vec![("Bo".into(), "Lovely & strange".into(), "p1".into())],
            }],
        };
        assert!(summary.is_worth_sending());
        let m = message("https://j.test", "a@b.c", "Ann", period, &summary, "tok");
        assert_eq!(
            m.subject,
            "Your week in writing: 2,400 words, 7 new comments, 2 tasks done"
        );
        for needle in [
            "You wrote 2,400 words.",
            "You wrote on 2 days of 7.",
            "Mon 900 · Tue – · Wed 1,500 · Thu – · Fri – · Sat – · Sun –",
            "Your weekly goal is 5,000: 2,600 to go next time.",
            "You met your daily goal of 1,000 on 1 day.",
            "Bo: \"Lovely & strange\"",
            "https://j.test/review/p1",
            "and 6 more",
            "[ ] Make chapter four colder",
            "https://j.test/summary-email/unsubscribe?token=tok",
        ] {
            assert!(m.body.contains(needle), "missing {needle:?} in\n{}", m.body);
        }
        let html = m.html.unwrap();
        assert!(html.contains("Tide &lt;draft&gt;") && html.contains("Lovely &amp; strange"));
        assert!(m.headers.iter().any(|(k, v)| k == "List-Unsubscribe"
            && v == "<https://j.test/api/summary-email/unsubscribe?token=tok>"));
    }

    #[test]
    fn an_empty_period_is_not_worth_an_email() {
        let summary = Summary {
            tasks: vec![BookTasks {
                work_id: "w1".into(),
                title: "Tide".into(),
                open: vec!["Still open".into()],
                open_count: 1,
                done: 0,
            }],
            ..Default::default()
        };
        assert!(!summary.is_worth_sending());
    }
}
