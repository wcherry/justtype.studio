//! The scheduled work behind moderation: telling people a restriction has
//! ended, applying decisions that came back from the external platform,
//! retrying checks it missed, flagging accounts that cross a threshold,
//! reminding closing accounts, and deleting them once their 30 days are up.
//!
//! Called from `jobs::tick`. The daily ones are claimed in `job_runs` like the
//! prompt jobs, so a restart or a second process runs them once.
use chrono::{DateTime, Duration, Utc};
use diesel::prelude::*;
use serde_json::json;

use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::enums::AuditAction;
use crate::domain::external_moderation::{self as external, Recommended};
use crate::domain::moderation::{self, Flag};
use crate::domain::notices;
use crate::domain::restrictions as rules;
use crate::error::{ApiError, ApiResult};
use crate::routes::audit::record_staff;
use crate::state::AppState;

pub const PURGE_ACCOUNTS: &str = "purge_accounts";
pub const CLOSING_REMINDERS: &str = "closing_reminders";

/// Who the log credits for what the scheduler does.
pub const SYSTEM: &str = "system";

fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Sends the *it has ended* notice for temporary restrictions that have run
/// out. Nothing needs to run for the restriction itself to stop applying.
pub fn expire_restrictions(conn: &mut Conn) -> ApiResult<usize> {
    let now = clock::now_iso();
    let ended: Vec<Restriction> = restrictions::table
        .filter(restrictions::kind.ne("warning"))
        .filter(restrictions::lifted_at.is_null())
        .filter(restrictions::ends_at.is_not_null())
        .filter(restrictions::ends_at.le(&now))
        .filter(restrictions::ended_notified_at.is_null())
        .select(Restriction::as_select())
        .load(conn)?;
    let n = ended.len();
    for r in ended {
        conn.transaction::<_, ApiError, _>(|conn| {
            diesel::update(restrictions::table.find(&r.id))
                .set(restrictions::ended_notified_at.eq(Some(clock::now_iso())))
                .execute(conn)?;
            notices::restriction_ended(conn, &r)
        })?;
    }
    Ok(n)
}

/// Deletes emailed copies of people's data once their link has expired, rows
/// and files both. A copy of someone's whole account should not outlive the
/// week it was made for.
pub fn sweep_exports(conn: &mut Conn) -> ApiResult<usize> {
    let now = clock::now_iso();
    let expired: Vec<(String, String)> = emailed_exports::table
        .filter(emailed_exports::expires_at.le(&now))
        .select((emailed_exports::id, emailed_exports::path))
        .load(conn)?;
    for (id, path) in &expired {
        if let Err(e) = std::fs::remove_file(path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(error = %e, %path, "could not remove an expired export");
                continue;
            }
        }
        diesel::delete(emailed_exports::table.find(id)).execute(conn)?;
    }
    Ok(expired.len())
}

/// Flags accounts that cross a threshold: distinct reporters, recent
/// removals, or a burst of comments in the first hour.
pub fn auto_flag_accounts(conn: &mut Conn) -> ApiResult<usize> {
    let t = moderation::thresholds(conn)?;
    let now = clock::now();
    let mut flagged = 0;
    let mut flag_account = |conn: &mut Conn, user_id: &str, what: String| -> ApiResult<()> {
        // Staff and the placeholder account are never flagged this way.
        let skip: Option<(bool, bool)> = users::table
            .find(user_id)
            .select((users::is_admin, users::is_moderator))
            .first(conn)
            .optional()?;
        match skip {
            None | Some((true, _)) | Some((_, true)) => return Ok(()),
            _ => {}
        }
        if user_id == crate::routes::privacy::FORMER_MEMBER {
            return Ok(());
        }
        let existing: Option<String> = moderation_items::table
            .filter(moderation_items::subject_type.eq("account"))
            .filter(moderation_items::subject_id.eq(user_id))
            .filter(moderation_items::state.eq("open"))
            .select(moderation_items::id)
            .first(conn)
            .optional()?;
        if existing.is_some() {
            return Ok(());
        }
        moderation::flag(
            conn,
            &Flag {
                subject_type: "account",
                subject_id: user_id,
                author_id: user_id,
                source: "threshold",
                reason: None,
                severity: 50,
                labels: Some(json!({ "threshold": what })),
                hold: false,
                new_report: false,
            },
        )?;
        flagged += 1;
        Ok(())
    };

    // Distinct reporters in the window, across every kind of report.
    let since = iso(now - Duration::days(t.reporter_days));
    let mut by_author: std::collections::HashMap<String, std::collections::HashSet<String>> =
        Default::default();
    for (author, reporter) in reports::table
        .filter(reports::created_at.ge(&since))
        .select((reports::subject_author_id, reports::reporter_id))
        .load::<(String, String)>(conn)?
    {
        by_author.entry(author).or_default().insert(reporter);
    }
    for (author, reporter) in comment_reports::table
        .inner_join(comments::table)
        .filter(comment_reports::created_at.ge(&since))
        .select((comments::author_id, comment_reports::reporter_id))
        .load::<(String, String)>(conn)?
    {
        by_author.entry(author).or_default().insert(reporter);
    }
    for (author, reporter) in prompt_critique_reports::table
        .inner_join(prompt_critiques::table)
        .filter(prompt_critique_reports::created_at.ge(&since))
        .select((
            prompt_critiques::author_id,
            prompt_critique_reports::reporter_id,
        ))
        .load::<(String, String)>(conn)?
    {
        by_author.entry(author).or_default().insert(reporter);
    }
    for (author, reporters) in by_author {
        if reporters.len() as i64 >= t.reporters {
            flag_account(
                conn,
                &author,
                format!(
                    "{} different people reported them in {} days",
                    reporters.len(),
                    t.reporter_days
                ),
            )?;
        }
    }

    // Removals in the window.
    let since = iso(now - Duration::days(t.removal_days));
    let removals: Vec<(String, i64)> = audit_events::table
        .filter(audit_events::action.eq(AuditAction::ContentRemoved.as_str()))
        .filter(audit_events::created_at.ge(&since))
        .filter(audit_events::subject_id.is_not_null())
        .group_by(audit_events::subject_id)
        .select((
            audit_events::subject_id.assume_not_null(),
            diesel::dsl::count(audit_events::id),
        ))
        .load(conn)?;
    for (author, n) in removals {
        if n >= t.removals {
            flag_account(
                conn,
                &author,
                format!("{n} posts removed in {} days", t.removal_days),
            )?;
        }
    }

    // A burst of comments from an account in its first hour.
    let recent: Vec<(String, String)> = users::table
        .filter(users::created_at.ge(iso(now - Duration::hours(24))))
        .select((users::id, users::created_at))
        .load(conn)?;
    for (id, created) in recent {
        let end = DateTime::parse_from_rfc3339(&created)
            .map(|c| iso(c.with_timezone(&Utc) + Duration::hours(1)))
            .unwrap_or_default();
        let n: i64 = comments::table
            .filter(comments::author_id.eq(&id))
            .filter(comments::created_at.ge(&created))
            .filter(comments::created_at.le(&end))
            .count()
            .get_result(conn)?;
        if n >= t.comments_first_hour {
            flag_account(conn, &id, format!("{n} comments in its first hour"))?;
        }
    }
    Ok(flagged)
}

/// Checks the posts the platform missed, with back-off, and tells it what we
/// decided. A platform that is down only delays this; it never stops anyone
/// posting.
pub async fn external_checks(state: &AppState) -> ApiResult<usize> {
    let secret = state.config.jwt_secret.clone();
    let (config, due, outbox) = blocking(&state.pool, |conn| {
        let config = moderation::external_config(conn)?;
        let now = clock::now_iso();
        let due: Vec<ExternalCheck> = external_checks::table
            .filter(external_checks::state.eq("pending"))
            .filter(external_checks::next_attempt_at.le(&now))
            .order(external_checks::created_at.asc())
            .limit(50)
            .select(ExternalCheck::as_select())
            .load(conn)?;
        let outbox: Vec<(String, String, String, i32)> = external_outbox::table
            .filter(external_outbox::sent_at.is_null())
            .filter(external_outbox::attempts.lt(10))
            .limit(50)
            .select((
                external_outbox::id,
                external_outbox::external_id,
                external_outbox::decision,
                external_outbox::attempts,
            ))
            .load(conn)?;
        Ok((config, due, outbox))
    })
    .await?;
    if !config.enabled() {
        return Ok(0);
    }
    let provider = config.provider(&secret)?;
    let mut done = 0;
    for check in due {
        let subject = {
            let (t, i) = (check.subject_type.clone(), check.subject_id.clone());
            blocking(&state.pool, move |conn| moderation::subject(conn, &t, &i)).await?
        };
        let Some(subject) = subject.filter(|s| s.public) else {
            // The thing is gone, or was never public: nothing to check.
            let id = check.id.clone();
            blocking(&state.pool, move |conn| {
                diesel::update(external_checks::table.find(&id))
                    .set(external_checks::state.eq("done"))
                    .execute(conn)?;
                Ok(())
            })
            .await?;
            continue;
        };
        let Some(surface) = subject.surface else {
            continue;
        };
        let item = external::CheckItem {
            surface: surface.as_str().into(),
            subject_id: subject.id.clone(),
            author_ref: external::author_ref(&secret, &subject.author_id),
            text: subject.text.clone(),
        };
        let attempt =
            tokio::time::timeout(std::time::Duration::from_secs(10), provider.check(&item)).await;
        let remove_as = config.remove_as.clone();
        let check_id = check.id.clone();
        let attempts = check.attempts + 1;
        let provider_name = provider.name().to_string();
        match attempt {
            Ok(Ok(verdict)) => {
                blocking(&state.pool, move |conn| {
                    conn.transaction::<_, ApiError, _>(|conn| {
                        let labels = json!({ "labels": verdict.labels });
                        diesel::update(external_checks::table.find(&check_id))
                            .set((
                                external_checks::state.eq("done"),
                                external_checks::attempts.eq(attempts),
                                external_checks::external_id.eq(verdict.external_id.clone()),
                                external_checks::recommended_action.eq(Some(
                                    match verdict.recommended {
                                        Recommended::Keep => "keep",
                                        Recommended::Flag => "flag",
                                        Recommended::Remove => "remove",
                                    }
                                    .to_string(),
                                )),
                                external_checks::labels.eq(Some(labels.to_string())),
                                external_checks::checked_at.eq(Some(clock::now_iso())),
                            ))
                            .execute(conn)?;
                        match verdict.recommended {
                            Recommended::Keep => {}
                            Recommended::Flag | Recommended::Remove => {
                                // It is already public, so a "block" can only
                                // be a hold now.
                                let hold = verdict.recommended == Recommended::Remove
                                    && (remove_as == "hold" || remove_as == "block");
                                moderation::record(
                                    conn,
                                    &moderation::Screen {
                                        hold,
                                        flags: vec![(
                                            "external".into(),
                                            if hold { 70 } else { 40 },
                                            Some(labels),
                                        )],
                                        ..Default::default()
                                    },
                                    &subject.kind,
                                    &subject.id,
                                    &subject.author_id,
                                    &provider_name,
                                )?;
                            }
                        }
                        Ok(())
                    })
                })
                .await?;
                done += 1;
            }
            Ok(Err(_)) | Err(_) => {
                // Back off: 1, 2, 4 … minutes up to an hour; give up after ten.
                let wait = Duration::minutes((1i64 << attempts.min(6)).min(60));
                blocking(&state.pool, move |conn| {
                    diesel::update(external_checks::table.find(&check_id))
                        .set((
                            external_checks::attempts.eq(attempts),
                            external_checks::state.eq(if attempts >= 10 {
                                "failed"
                            } else {
                                "pending"
                            }),
                            external_checks::next_attempt_at.eq(iso(clock::now() + wait)),
                        ))
                        .execute(conn)?;
                    Ok(())
                })
                .await?;
            }
        }
    }
    for (id, external_id, decision, attempts) in outbox {
        let sent = provider.report_decision(&external_id, &decision).await;
        blocking(&state.pool, move |conn| {
            match sent {
                Ok(()) => diesel::update(external_outbox::table.find(&id))
                    .set(external_outbox::sent_at.eq(Some(clock::now_iso())))
                    .execute(conn)?,
                Err(_) => diesel::update(external_outbox::table.find(&id))
                    .set(external_outbox::attempts.eq(attempts + 1))
                    .execute(conn)?,
            };
            Ok(())
        })
        .await?;
    }
    Ok(done)
}

/// The 7-day and 1-day reminders for read-only accounts counting down to
/// deletion. Once each, per account.
pub fn send_closing_reminders(conn: &mut Conn, now: DateTime<Utc>) -> ApiResult<usize> {
    let rows: Vec<(String, Option<String>)> = users::table
        .filter(users::purge_at.is_not_null())
        .select((users::id, users::purge_at))
        .load(conn)?;
    let mut sent = 0;
    for (user_id, purge_at) in rows {
        let Some(purge_at) = purge_at else { continue };
        let Ok(purge) = DateTime::parse_from_rfc3339(&purge_at).map(|t| t.with_timezone(&Utc))
        else {
            continue;
        };
        let left = purge - now;
        let active = rules::active(conn, &user_id)?;
        let Some(r) = active
            .iter()
            .find(|r| r.kind == "read_only" && r.scope == "site")
            .cloned()
        else {
            continue;
        };
        let tier = if left <= Duration::days(1) {
            1
        } else if left <= Duration::days(7) {
            7
        } else {
            continue;
        };
        if left <= Duration::zero() {
            continue;
        }
        if !crate::jobs::claim(conn, CLOSING_REMINDERS, &format!("{user_id}:{tier}"), now)? {
            continue;
        }
        let outcome = notices::closing_reminder(conn, &r, &purge_at, tier);
        crate::jobs::finish(
            conn,
            CLOSING_REMINDERS,
            &format!("{user_id}:{tier}"),
            outcome.as_ref().err().map(|e| e.to_string()),
        )?;
        outcome?;
        sent += 1;
    }
    Ok(sent)
}

/// What a purge removed, to finish outside the transaction.
pub struct Purged {
    pub user_id: String,
    pub work_files: Vec<String>,
}

/// Deletes one closed account, if it is still due. Everything is checked
/// again inside the transaction: the closing restriction must still be in
/// force, `purge_at` must have passed, and no appeal may be open.
pub fn purge_one(conn: &mut Conn, user_id: &str, now: DateTime<Utc>) -> ApiResult<Option<Purged>> {
    conn.transaction::<_, ApiError, _>(|conn| {
        let Some(user): Option<User> = users::table
            .find(user_id)
            .select(User::as_select())
            .first(conn)
            .optional()?
        else {
            return Ok(None);
        };
        let purge_at: Option<String> = users::table
            .find(user_id)
            .select(users::purge_at)
            .first(conn)?;
        let Some(purge_at) = purge_at.filter(|p| p.as_str() <= iso(now).as_str()) else {
            return Ok(None);
        };
        let active = rules::active(conn, user_id)?;
        let Some(closing) = rules::closing(&active).cloned() else {
            return Ok(None);
        };
        let waiting: i64 = appeals::table
            .filter(appeals::restriction_id.eq(&closing.id))
            .filter(appeals::state.eq("open"))
            .count()
            .get_result(conn)?;
        if waiting > 0 {
            return Ok(None);
        }
        let counts = json!({
            "works": works::table.filter(works::owner_id.eq(user_id)).count().get_result::<i64>(conn)?,
            "comments": comments::table.filter(comments::author_id.eq(user_id)).count().get_result::<i64>(conn)?,
            "entries": prompt_entries::table.filter(prompt_entries::author_id.eq(user_id)).count().get_result::<i64>(conn)?,
        });
        let removed = crate::routes::privacy::remove_account(conn, &user, true)?;
        for (work_id, title, heir) in &removed.handed_over {
            notices::book_handed_over(conn, heir, work_id, title)?;
            crate::routes::audit::record(
                conn,
                Some(SYSTEM),
                Some(work_id),
                AuditAction::WorkHandedOver,
                &format!("{title} passed to its earliest collaborator"),
            )?;
        }
        // Items about what is gone keep their decision and lose the content.
        diesel::update(
            moderation_items::table
                .filter(moderation_items::subject_author_id.eq(user_id))
                .filter(moderation_items::state.eq_any(["open", "held"])),
        )
        .set((
            moderation_items::state.eq("dismissed"),
            moderation_items::decision.eq(Some("account_purged".to_string())),
            moderation_items::decided_by.eq(Some(SYSTEM.to_string())),
            moderation_items::decided_at.eq(Some(iso(now))),
        ))
        .execute(conn)?;
        // The platform is told to forget its opaque id for them, if it can.
        record_staff(
            conn,
            SYSTEM,
            Some(user_id),
            AuditAction::AccountPurged,
            &format!(
                "scheduled deletion on {purge_at}: {} work(s) deleted, {} handed over; {counts}",
                removed.deleted.len(),
                removed.handed_over.len()
            ),
            Some("the 30 days after a closing restriction have passed"),
        )?;
        Ok(Some(Purged {
            user_id: user_id.to_string(),
            work_files: removed.deleted.into_iter().map(|w| w.0).collect(),
        }))
    })
}

/// Deletes every account whose time has come. Daily, keyed by day.
pub async fn purge_accounts(state: &AppState, now: DateTime<Utc>) -> ApiResult<usize> {
    let key = clock::day_string(clock::prompt_day(now));
    let claim_key = key.clone();
    let taken = blocking(&state.pool, move |conn| {
        crate::jobs::claim(conn, PURGE_ACCOUNTS, &claim_key, now)
    })
    .await?;
    if !taken {
        return Ok(0);
    }
    let due: ApiResult<Vec<String>> = blocking(&state.pool, move |conn| {
        Ok(users::table
            .filter(users::purge_at.is_not_null())
            .filter(users::purge_at.le(iso(now)))
            .select(users::id)
            .load(conn)?)
    })
    .await;
    let mut purged = Vec::new();
    let mut outcome: ApiResult<()> = Ok(());
    match due {
        Ok(ids) => {
            for id in ids {
                let one = blocking(&state.pool, {
                    let id = id.clone();
                    move |conn| purge_one(conn, &id, now)
                })
                .await;
                match one {
                    Ok(Some(p)) => purged.push(p),
                    Ok(None) => {}
                    Err(e) => {
                        tracing::error!(user = %id, error = %e, "could not purge an account");
                        outcome = Err(e);
                    }
                }
            }
        }
        Err(e) => outcome = Err(e),
    }
    // The rows are gone; now the files and the platform's copy.
    let secret = state.config.jwt_secret.clone();
    let config = blocking(&state.pool, moderation::external_config).await?;
    for p in &purged {
        for work_id in &p.work_files {
            if let Err(e) = state.store.delete_work(work_id).await {
                tracing::error!(%work_id, error = %e, "account purged but its work's files remain");
            }
        }
        if config.enabled() {
            if let Ok(provider) = config.provider(&secret) {
                let reference = external::author_ref(&secret, &p.user_id);
                if let Err(e) = provider.forget_author(&reference).await {
                    tracing::warn!(error = %e, "the platform could not forget a purged account");
                }
            }
        }
    }
    let error = outcome.as_ref().err().map(|e| e.to_string());
    blocking(&state.pool, move |conn| {
        crate::jobs::finish(conn, PURGE_ACCOUNTS, &key, error)
    })
    .await?;
    outcome?;
    Ok(purged.len())
}
