//! Moderation: what is flagged, what staff do about it, and the screening that
//! happens when something public is posted.
//!
//! Everything that flags a thing (a report, the word filter, the external
//! platform, a threshold) goes through [`flag`], which keeps one row per
//! subject in `moderation_items`: twenty reports are one queue item. Everything
//! that decides one (a moderator, or an event from the platform) goes through
//! [`remove`], [`restore`], [`hold`], [`release`] or [`dismiss`], so the two
//! kinds of decision are applied by the same code and recorded the same way.
//!
//! Only public things are subjects (Principle 1). There is no subject type for
//! a manuscript, a draft, a notebook or an unsubmitted answer.
use diesel::prelude::*;
use serde_json::{json, Value};

use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::enums::AuditAction;
use crate::domain::external_moderation::{self as external, Recommended};
use crate::domain::filter::{FilterAction, Surface};
use crate::domain::{doc, new_id};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Subjects that carry a `moderation_state` and so can be removed or held.
pub const CONTENT: &[&str] = &["comment", "question", "answer", "entry", "critique"];
/// How soon a reason must be seen. Self-harm and illegal first.
pub fn severity(reason: &str) -> i32 {
    match reason {
        "self_harm" => 100,
        "illegal" => 90,
        "hate" => 70,
        "harassment" => 60,
        "sexual_content" => 50,
        "abuse" => 40,
        "impersonation" => 40,
        "spam" => 20,
        _ => 10,
    }
}

// --- settings ----------------------------------------------------------------

pub fn setting(conn: &mut Conn, key: &str) -> ApiResult<Option<Value>> {
    let raw: Option<String> = moderation_settings::table
        .find(key)
        .select(moderation_settings::value)
        .first(conn)
        .optional()?;
    Ok(raw.and_then(|r| serde_json::from_str(&r).ok()))
}

pub fn put_setting(conn: &mut Conn, key: &str, value: &Value, by: &str) -> ApiResult<()> {
    let row = (
        moderation_settings::key.eq(key),
        moderation_settings::value.eq(value.to_string()),
        moderation_settings::updated_by.eq(Some(by.to_string())),
        moderation_settings::updated_at.eq(clock::now_iso()),
    );
    diesel::replace_into(moderation_settings::table)
        .values(&row)
        .execute(conn)?;
    Ok(())
}

pub fn external_config(conn: &mut Conn) -> ApiResult<external::ExternalConfig> {
    Ok(setting(conn, "external")?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

/// The numbers `auto_flag_accounts` compares against.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Thresholds {
    pub reporters: i64,
    pub reporter_days: i64,
    pub removals: i64,
    pub removal_days: i64,
    pub comments_first_hour: i64,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            reporters: 3,
            reporter_days: 7,
            removals: 2,
            removal_days: 30,
            comments_first_hour: 5,
        }
    }
}

pub fn thresholds(conn: &mut Conn) -> ApiResult<Thresholds> {
    Ok(setting(conn, "thresholds")?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

/// The escalation ladder shown as a hint when restricting: `kind:scope:days`,
/// `warning`, or `permanent`.
pub fn ladder(conn: &mut Conn) -> ApiResult<Vec<String>> {
    Ok(setting(conn, "ladder")?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_else(|| {
            vec![
                "warning".into(),
                "suspend:discussion:3".into(),
                "suspend:site:30".into(),
                "ban:site:permanent".into(),
            ]
        }))
}

// --- subjects ----------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Subject {
    pub kind: String,
    pub id: String,
    pub author_id: String,
    /// The words, as plain text, for the queue and for the platform.
    pub text: String,
    pub surface: Option<Surface>,
    /// Where it sits: the publication, the prompt, the thing replied to.
    pub context: String,
    pub moderation_state: String,
    /// Whether anyone but the author can see it (a draft entry is private).
    pub public: bool,
}

pub fn subject(conn: &mut Conn, kind: &str, id: &str) -> ApiResult<Option<Subject>> {
    Ok(match kind {
        "comment" => comments::table
            .find(id)
            .select(Comment::as_select())
            .first(conn)
            .optional()?
            .map(|c| {
                let body = c.original_body.clone().unwrap_or(c.body.clone());
                Subject {
                    kind: kind.into(),
                    id: c.id,
                    author_id: c.author_id,
                    text: body,
                    surface: Some(Surface::Comment),
                    context: format!("publication {}", c.publication_id),
                    moderation_state: c.moderation_state,
                    public: c.state != "deleted",
                }
            }),
        "question" => review_questions::table
            .find(id)
            .inner_join(publications::table.inner_join(works::table))
            .select((ReviewQuestion::as_select(), works::owner_id))
            .first::<(ReviewQuestion, String)>(conn)
            .optional()?
            .map(|(q, owner)| Subject {
                kind: kind.into(),
                id: q.id,
                author_id: owner,
                text: q.prompt,
                surface: Some(Surface::Question),
                context: format!("publication {}", q.publication_id),
                moderation_state: q.moderation_state,
                public: true,
            }),
        "answer" => review_answers::table
            .find(id)
            .inner_join(review_questions::table)
            .select((ReviewAnswer::as_select(), review_questions::prompt))
            .first::<(ReviewAnswer, String)>(conn)
            .optional()?
            .map(|(a, prompt)| Subject {
                kind: kind.into(),
                id: a.id,
                author_id: a.user_id,
                text: a.body.unwrap_or_default(),
                surface: Some(Surface::Answer),
                context: format!("answer to “{prompt}”"),
                moderation_state: a.moderation_state,
                public: true,
            }),
        "entry" => prompt_entries::table
            .find(id)
            .select(PromptEntry::as_select())
            .first(conn)
            .optional()?
            .map(|e| {
                let parsed: Value = serde_json::from_str(&e.body).unwrap_or(Value::Null);
                Subject {
                    kind: kind.into(),
                    id: e.id,
                    author_id: e.author_id,
                    text: doc::plain_text(&parsed),
                    surface: Some(Surface::Entry),
                    context: format!("prompt {}", e.prompt_id),
                    moderation_state: e.moderation_state,
                    // An unsubmitted answer is private and never moderated.
                    public: e.state == "submitted",
                }
            }),
        "critique" => prompt_critiques::table
            .find(id)
            .select(PromptCritique::as_select())
            .first(conn)
            .optional()?
            .map(|c| Subject {
                kind: kind.into(),
                id: c.id,
                author_id: c.author_id,
                text: c.original_body.clone().unwrap_or(c.body),
                surface: Some(Surface::Critique),
                context: format!("entry {}", c.entry_id),
                moderation_state: c.moderation_state,
                public: c.deleted_at.is_none(),
            }),
        "profile" | "account" => users::table
            .find(id)
            .select((
                users::id,
                users::display_name,
                users::bio,
                users::about_author,
            ))
            .first::<(String, String, Option<String>, Option<String>)>(conn)
            .optional()?
            .map(|(uid, name, bio, about)| Subject {
                kind: kind.into(),
                id: uid.clone(),
                author_id: uid,
                text: [Some(name), bio, about]
                    .into_iter()
                    .flatten()
                    .filter(|t| !t.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
                surface: None,
                context: String::new(),
                moderation_state: "visible".into(),
                public: true,
            }),
        "publication" => publications::table
            .find(id)
            .inner_join(works::table)
            .select((
                publications::id,
                works::owner_id,
                works::title,
                works::synopsis,
            ))
            .first::<(String, String, String, Option<String>)>(conn)
            .optional()?
            .map(|(pid, owner, title, synopsis)| Subject {
                kind: kind.into(),
                id: pid,
                author_id: owner,
                text: format!("{title}\n{}", synopsis.unwrap_or_default()),
                surface: Some(Surface::PublicationTitle),
                context: String::new(),
                moderation_state: "visible".into(),
                public: true,
            }),
        _ => None,
    })
}

/// Sets the moderation state of a content subject.
fn set_state(conn: &mut Conn, kind: &str, id: &str, state: &str) -> ApiResult<()> {
    let changed = match kind {
        "comment" => diesel::update(comments::table.find(id))
            .set(comments::moderation_state.eq(state))
            .execute(conn)?,
        "question" => diesel::update(review_questions::table.find(id))
            .set(review_questions::moderation_state.eq(state))
            .execute(conn)?,
        "answer" => diesel::update(review_answers::table.find(id))
            .set(review_answers::moderation_state.eq(state))
            .execute(conn)?,
        "entry" => diesel::update(prompt_entries::table.find(id))
            .set(prompt_entries::moderation_state.eq(state))
            .execute(conn)?,
        "critique" => diesel::update(prompt_critiques::table.find(id))
            .set(prompt_critiques::moderation_state.eq(state))
            .execute(conn)?,
        _ => {
            return Err(ApiError::BadRequest(format!(
                "a {kind} can't be removed or held — restrict or edit its author instead"
            )))
        }
    };
    if changed == 0 {
        return Err(ApiError::NotFound(format!("no such {kind}")));
    }
    Ok(())
}

// --- flagging ----------------------------------------------------------------

pub struct Flag<'a> {
    pub subject_type: &'a str,
    pub subject_id: &'a str,
    pub author_id: &'a str,
    /// `report`, `word_filter`, `external` or `threshold`.
    pub source: &'a str,
    pub reason: Option<&'a str>,
    pub severity: i32,
    pub labels: Option<Value>,
    /// Whether the thing is held for review (not yet public).
    pub hold: bool,
    /// A report from someone who hadn't reported it before.
    pub new_report: bool,
}

/// Creates the queue item for a subject, or bumps the one it has. A dismissed
/// or actioned item that is flagged again reopens.
pub fn flag(conn: &mut Conn, f: &Flag) -> ApiResult<ModerationItem> {
    let now = clock::now_iso();
    let existing: Option<ModerationItem> = moderation_items::table
        .filter(moderation_items::subject_type.eq(f.subject_type))
        .filter(moderation_items::subject_id.eq(f.subject_id))
        .select(ModerationItem::as_select())
        .first(conn)
        .optional()?;
    match existing {
        None => {
            diesel::insert_into(moderation_items::table)
                .values((
                    moderation_items::id.eq(new_id()),
                    moderation_items::subject_type.eq(f.subject_type),
                    moderation_items::subject_id.eq(f.subject_id),
                    moderation_items::subject_author_id.eq(f.author_id),
                    moderation_items::state.eq(if f.hold { "held" } else { "open" }),
                    moderation_items::sources.eq(json!([f.source]).to_string()),
                    moderation_items::report_count.eq(f.new_report as i32),
                    moderation_items::top_reason.eq(f.reason),
                    moderation_items::severity.eq(f.severity),
                    moderation_items::labels.eq(f.labels.as_ref().map(|l| l.to_string())),
                    moderation_items::first_flagged_at.eq(&now),
                    moderation_items::last_flagged_at.eq(&now),
                ))
                .execute(conn)?;
        }
        Some(item) => {
            let mut sources: Vec<String> = serde_json::from_str(&item.sources).unwrap_or_default();
            if !sources.iter().any(|s| s == f.source) {
                sources.push(f.source.to_string());
            }
            let severity = item.severity.max(f.severity);
            let top_reason = if f.severity >= item.severity && f.reason.is_some() {
                f.reason.map(str::to_string)
            } else {
                item.top_reason.clone()
            };
            let state = if f.hold {
                "held".to_string()
            } else if matches!(item.state.as_str(), "dismissed" | "actioned") {
                "open".to_string()
            } else {
                item.state.clone()
            };
            let reopened = state != item.state && state == "open";
            diesel::update(moderation_items::table.find(&item.id))
                .set((
                    moderation_items::state.eq(&state),
                    moderation_items::sources.eq(json!(sources).to_string()),
                    moderation_items::report_count.eq(item.report_count + f.new_report as i32),
                    moderation_items::top_reason.eq(top_reason),
                    moderation_items::severity.eq(severity),
                    moderation_items::labels.eq(f
                        .labels
                        .as_ref()
                        .map(|l| l.to_string())
                        .or(item.labels.clone())),
                    moderation_items::last_flagged_at.eq(&now),
                ))
                .execute(conn)?;
            if reopened {
                diesel::update(moderation_items::table.find(&item.id))
                    .set((
                        moderation_items::decision.eq(None::<String>),
                        moderation_items::decided_by.eq(None::<String>),
                        moderation_items::decided_at.eq(None::<String>),
                    ))
                    .execute(conn)?;
            }
        }
    }
    Ok(moderation_items::table
        .filter(moderation_items::subject_type.eq(f.subject_type))
        .filter(moderation_items::subject_id.eq(f.subject_id))
        .select(ModerationItem::as_select())
        .first(conn)?)
}

pub fn item(conn: &mut Conn, id: &str) -> ApiResult<ModerationItem> {
    moderation_items::table
        .find(id)
        .select(ModerationItem::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("no such queue item".into()))
}

// --- decisions -----------------------------------------------------------------

/// Who decided: a staff member's user id, or `external:<provider>:<moderator>`.
pub fn is_external(actor: &str) -> bool {
    actor.starts_with("external:")
}

fn decide(
    conn: &mut Conn,
    item: &ModerationItem,
    actor: &str,
    state: &str,
    decision: &str,
) -> ApiResult<()> {
    diesel::update(moderation_items::table.find(&item.id))
        .set((
            moderation_items::state.eq(state),
            moderation_items::decision.eq(Some(decision.to_string())),
            moderation_items::decided_by.eq(Some(actor.to_string())),
            moderation_items::decided_at.eq(Some(clock::now_iso())),
            moderation_items::claimed_by.eq(None::<String>),
            moderation_items::claimed_until.eq(None::<String>),
        ))
        .execute(conn)?;
    Ok(())
}

/// Tells the platform what we decided, if the item came from it, so its
/// review queue closes too. Written to an outbox the job drains.
fn close_on_platform(
    conn: &mut Conn,
    item: &ModerationItem,
    actor: &str,
    decision: &str,
) -> ApiResult<()> {
    if is_external(actor) {
        return Ok(());
    }
    let checks: Vec<(String, Option<String>)> = external_checks::table
        .filter(external_checks::subject_type.eq(&item.subject_type))
        .filter(external_checks::subject_id.eq(&item.subject_id))
        .filter(external_checks::external_id.is_not_null())
        .select((external_checks::provider, external_checks::external_id))
        .load(conn)?;
    for (provider, external_id) in checks {
        if let Some(external_id) = external_id {
            diesel::insert_into(external_outbox::table)
                .values((
                    external_outbox::id.eq(new_id()),
                    external_outbox::provider.eq(provider),
                    external_outbox::external_id.eq(external_id),
                    external_outbox::decision.eq(decision),
                    external_outbox::attempts.eq(0),
                    external_outbox::created_at.eq(clock::now_iso()),
                ))
                .execute(conn)?;
        }
    }
    Ok(())
}

pub fn dismiss(
    conn: &mut Conn,
    item_id: &str,
    actor: &str,
    reason: &str,
) -> ApiResult<ModerationItem> {
    let item = item(conn, item_id)?;
    // Dismissing something held says it is fine: it goes live.
    if CONTENT.contains(&item.subject_type.as_str()) {
        if let Some(s) = subject(conn, &item.subject_type, &item.subject_id)? {
            if s.moderation_state == "held" {
                set_state(conn, &item.subject_type, &item.subject_id, "visible")?;
            }
        }
    }
    decide(conn, &item, actor, "dismissed", "dismissed")?;
    close_on_platform(conn, &item, actor, "safe")?;
    crate::routes::audit::record_staff(
        conn,
        actor,
        Some(&item.subject_author_id),
        AuditAction::ReportDismissed,
        &format!("{} {}", item.subject_type, item.subject_id),
        Some(reason),
    )?;
    self::item(conn, item_id)
}

pub fn remove(
    conn: &mut Conn,
    item_id: &str,
    actor: &str,
    reason: &str,
) -> ApiResult<ModerationItem> {
    let item = item(conn, item_id)?;
    set_state(conn, &item.subject_type, &item.subject_id, "removed")?;
    decide(conn, &item, actor, "actioned", "removed")?;
    close_on_platform(conn, &item, actor, "removed")?;
    crate::routes::audit::record_staff(
        conn,
        actor,
        Some(&item.subject_author_id),
        AuditAction::ContentRemoved,
        &format!("{} {}", item.subject_type, item.subject_id),
        Some(reason),
    )?;
    // The author is told which part of the guidelines it broke.
    crate::routes::inbox::notify(
        conn,
        &item.subject_author_id,
        "moderation.content_removed",
        (&item.subject_type, &item.subject_id),
        None,
        None,
        &json!({
            "text": format!(
                "A moderator removed your {}. {}",
                noun(&item.subject_type),
                reason
            ),
        })
        .to_string(),
    )?;
    self::item(conn, item_id)
}

pub fn restore(
    conn: &mut Conn,
    item_id: &str,
    actor: &str,
    reason: &str,
) -> ApiResult<ModerationItem> {
    let item = item(conn, item_id)?;
    set_state(conn, &item.subject_type, &item.subject_id, "visible")?;
    decide(conn, &item, actor, "actioned", "restored")?;
    close_on_platform(conn, &item, actor, "restored")?;
    crate::routes::audit::record_staff(
        conn,
        actor,
        Some(&item.subject_author_id),
        AuditAction::ContentRestored,
        &format!("{} {}", item.subject_type, item.subject_id),
        Some(reason),
    )?;
    self::item(conn, item_id)
}

pub fn hold(
    conn: &mut Conn,
    item_id: &str,
    actor: &str,
    reason: &str,
) -> ApiResult<ModerationItem> {
    let item = item(conn, item_id)?;
    set_state(conn, &item.subject_type, &item.subject_id, "held")?;
    diesel::update(moderation_items::table.find(&item.id))
        .set(moderation_items::state.eq("held"))
        .execute(conn)?;
    crate::routes::audit::record_staff(
        conn,
        actor,
        Some(&item.subject_author_id),
        AuditAction::ContentHeld,
        &format!("{} {}", item.subject_type, item.subject_id),
        Some(reason),
    )?;
    self::item(conn, item_id)
}

pub fn release(
    conn: &mut Conn,
    item_id: &str,
    actor: &str,
    reason: &str,
) -> ApiResult<ModerationItem> {
    let item = item(conn, item_id)?;
    set_state(conn, &item.subject_type, &item.subject_id, "visible")?;
    decide(conn, &item, actor, "actioned", "released")?;
    close_on_platform(conn, &item, actor, "safe")?;
    crate::routes::audit::record_staff(
        conn,
        actor,
        Some(&item.subject_author_id),
        AuditAction::ContentRestored,
        &format!(
            "{} {} (released from hold)",
            item.subject_type, item.subject_id
        ),
        Some(reason),
    )?;
    self::item(conn, item_id)
}

fn noun(kind: &str) -> &'static str {
    match kind {
        "comment" => "comment",
        "question" => "question",
        "answer" => "answer",
        "entry" => "Daily Prompts answer",
        "critique" => "critique",
        _ => "post",
    }
}

/// Removes everything public an account has said, leaving the account. For
/// spam accounts that are restricted rather than deleted.
pub fn remove_all_discussion(conn: &mut Conn, user_id: &str) -> ApiResult<usize> {
    let mut count = 0;
    count += diesel::update(comments::table.filter(comments::author_id.eq(user_id)))
        .set(comments::moderation_state.eq("removed"))
        .execute(conn)?;
    count += diesel::update(review_answers::table.filter(review_answers::user_id.eq(user_id)))
        .set(review_answers::moderation_state.eq("removed"))
        .execute(conn)?;
    count +=
        diesel::update(prompt_critiques::table.filter(prompt_critiques::author_id.eq(user_id)))
            .set(prompt_critiques::moderation_state.eq("removed"))
            .execute(conn)?;
    count += diesel::update(
        prompt_entries::table
            .filter(prompt_entries::author_id.eq(user_id))
            .filter(prompt_entries::state.eq("submitted")),
    )
    .set(prompt_entries::moderation_state.eq("removed"))
    .execute(conn)?;
    let owned_pubs: Vec<String> = publications::table
        .inner_join(works::table)
        .filter(works::owner_id.eq(user_id))
        .select(publications::id)
        .load(conn)?;
    count += diesel::update(
        review_questions::table.filter(review_questions::publication_id.eq_any(owned_pubs)),
    )
    .set(review_questions::moderation_state.eq("removed"))
    .execute(conn)?;
    Ok(count)
}

// --- screening a post ------------------------------------------------------------

/// What screening decided about one piece of text, to act on after it is saved.
#[derive(Debug, Clone, Default)]
pub struct Screen {
    /// The text to store: the original, or with masked terms replaced.
    pub text: String,
    /// The words as typed, kept for staff, when they were masked.
    pub original: Option<String>,
    /// Saved as `held`: not public until a person releases it.
    pub hold: bool,
    pub flags: Vec<(String, i32, Option<Value>)>,
    /// The platform couldn't be reached in time: check again later.
    pub external_pending: bool,
}

impl Screen {
    /// The result for several fields posted together (a profile): the
    /// strongest of each.
    pub fn merge(mut self, other: Screen) -> Screen {
        self.hold |= other.hold;
        self.flags.extend(other.flags);
        self.external_pending |= other.external_pending;
        self
    }
}

pub const BLOCKED: &str = "That contains a word that isn't allowed here";

/// Screens `text` for `surface`. The word filter runs first, in process; if it
/// blocks, nothing is sent to the platform. A refusal is an error; everything
/// else is returned for [`record`] to act on once the post has an id.
pub async fn screen(
    state: &AppState,
    surface: Surface,
    author_id: &str,
    text: &str,
) -> ApiResult<Screen> {
    let mut out = Screen {
        text: text.to_string(),
        ..Default::default()
    };
    if text.trim().is_empty() {
        return Ok(out);
    }
    let filter = state.moderation.clone();
    let owned = text.to_string();
    let (verdict, config) = blocking(&state.pool, move |conn| {
        let compiled = filter.get(conn)?;
        Ok((compiled.check(surface, &owned), external_config(conn)?))
    })
    .await?;
    match verdict.action {
        Some(FilterAction::Block) => return Err(ApiError::BadRequest(BLOCKED.into())),
        Some(FilterAction::Hold) => {
            out.hold = true;
            out.flags
                .push(("word_filter".into(), 60, Some(json!({ "action": "hold" }))));
        }
        Some(FilterAction::Flag) => {
            out.flags
                .push(("word_filter".into(), 30, Some(json!({ "action": "flag" }))));
        }
        Some(FilterAction::Mask) => {
            // A document can't be rewritten safely, so on a Daily Prompts
            // answer a mask is a flag.
            if surface == Surface::Entry {
                out.flags
                    .push(("word_filter".into(), 20, Some(json!({ "action": "mask" }))));
            } else if let Some(masked) = verdict.masked {
                out.original = Some(text.to_string());
                out.text = masked;
                out.flags
                    .push(("word_filter".into(), 10, Some(json!({ "action": "mask" }))));
            }
        }
        None => {}
    }

    // The platform, with a timeout: it being slow or down never stops a post.
    if config.checks(surface.as_str()) {
        let secret = state.config.jwt_secret.clone();
        let provider = config.provider(&secret)?;
        let item = external::CheckItem {
            surface: surface.as_str().into(),
            subject_id: new_id(),
            author_ref: external::author_ref(&secret, author_id),
            text: text.to_string(),
        };
        match tokio::time::timeout(external::CHECK_TIMEOUT, provider.check(&item)).await {
            Ok(Ok(verdict)) => match verdict.recommended {
                Recommended::Keep => {}
                Recommended::Flag => out.flags.push((
                    "external".into(),
                    40,
                    Some(json!({ "labels": verdict.labels })),
                )),
                Recommended::Remove if config.remove_as == "block" => {
                    return Err(ApiError::BadRequest(BLOCKED.into()))
                }
                Recommended::Remove => {
                    out.hold = true;
                    out.flags.push((
                        "external".into(),
                        70,
                        Some(json!({ "labels": verdict.labels, "recommended": "remove" })),
                    ));
                }
            },
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "the moderation platform failed a check");
                out.external_pending = true;
            }
            Err(_) => out.external_pending = true,
        }
    }
    Ok(out)
}

/// Acts on what screening found, in the transaction that saved the post:
/// queue items for flags and holds, the held state on the row, and a pending
/// platform check if the platform was out of reach.
pub fn record(
    conn: &mut Conn,
    screen: &Screen,
    subject_type: &str,
    subject_id: &str,
    author_id: &str,
    provider: &str,
) -> ApiResult<()> {
    if screen.hold && CONTENT.contains(&subject_type) {
        set_state(conn, subject_type, subject_id, "held")?;
    }
    for (source, severity, labels) in &screen.flags {
        flag(
            conn,
            &Flag {
                subject_type,
                subject_id,
                author_id,
                source,
                reason: None,
                severity: *severity,
                labels: labels.clone(),
                hold: screen.hold,
                new_report: false,
            },
        )?;
    }
    if screen.external_pending {
        diesel::insert_into(external_checks::table)
            .values((
                external_checks::id.eq(new_id()),
                external_checks::subject_type.eq(subject_type),
                external_checks::subject_id.eq(subject_id),
                external_checks::provider.eq(provider),
                external_checks::state.eq("pending"),
                external_checks::attempts.eq(0),
                external_checks::created_at.eq(clock::now_iso()),
                external_checks::next_attempt_at.eq(clock::now_iso()),
            ))
            .execute(conn)?;
    }
    Ok(())
}

/// For handlers: screen, then (after saving) record. The provider name for a
/// pending check comes from the settings.
pub fn provider_name(conn: &mut Conn) -> ApiResult<String> {
    Ok(external_config(conn)?.provider)
}

/// Whether the viewer is site staff, who see held things in context.
pub fn is_staff(conn: &mut Conn, viewer: Option<&str>) -> ApiResult<bool> {
    let Some(id) = viewer else { return Ok(false) };
    Ok(users::table
        .find(id)
        .select((users::is_admin, users::is_moderator))
        .first::<(bool, bool)>(conn)
        .optional()?
        .is_some_and(|(a, m)| a || m))
}

/// What a viewer sees of something moderation may have hidden: `None` to leave
/// it out, `Some(true)` to show it as a placeholder (removed), `Some(false)`
/// to show it as it is.
///
/// A held thing is visible to its author and to staff, and to no one else.
/// Anything by an account that is hidden (deactivated, or permanently banned)
/// is left out of every public read.
pub fn visibility(
    state: &str,
    author_id: &str,
    viewer: Option<&str>,
    staff: bool,
    hidden_authors: &[String],
) -> Option<bool> {
    if hidden_authors.iter().any(|h| h == author_id) {
        return None;
    }
    match state {
        "removed" => Some(true),
        "held" if staff || viewer == Some(author_id) => Some(false),
        "held" => None,
        _ => Some(false),
    }
}
