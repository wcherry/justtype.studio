//! Reviews: a scene, chapter, part or the whole book put in front of readers
//! from the outline.
//!
//! A review is a publication of a snapshot, made for the writer rather than by
//! them. Marking something for review uses the most recent snapshot of it if
//! the draft has not moved on since, and freezes a new one if it has; bringing
//! a review up to date does the same and points the review at the result. The
//! comments readers left stay with the review, each saying which version it was
//! written on.
//!
//! The audience is everyone signed in, or the people picked — by name, or by
//! address for someone who has no account yet. Revoking closes it to them and
//! keeps every comment for the writers; it can be opened again.
//!
//! Owners and writers may do all of this: a writer handed some chapters only
//! for those chapters.
use std::collections::HashSet;

use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::Duration;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::AuthUser;
use crate::db::blocking;
use crate::db::models::*;
use crate::db::schema::{
    comments, publication_invites, publications, reading_progress, users, version_entries, versions,
};
use crate::domain::enums::{
    ActivityKind, AuditAction, PublicationScope, ReviewAudience, ReviewState,
};
use crate::domain::slice::{self, Slice};
use crate::domain::{moderation, new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::mail::Message;
use crate::state::AppState;

use super::support::{self, Ability, AuthorCard};
use super::works::{self, OutlineResponse};

/// How long a review invitation keeps trying to reach a mailbox.
const GIVE_UP_DAYS: i64 = 3;

// --- what a review covers -------------------------------------------------

/// One scene as a review sees it: enough to tell whether two copies would read
/// the same.
#[derive(Debug, PartialEq, Eq)]
struct Frozen {
    chapter_id: String,
    chapter_title: String,
    scene_id: String,
    scene_title: String,
    content_hash: String,
}

/// The book with untitled chapters and scenes named for where they fall, as a
/// snapshot names them.
fn named_outline(conn: &mut crate::db::Conn, work_id: &str) -> ApiResult<OutlineResponse> {
    Ok(works::with_placeholder_names(works::load_outline(
        conn, work_id,
    )?))
}

fn order_of(outline: &OutlineResponse) -> Vec<(&str, &str)> {
    outline
        .chapters
        .iter()
        .map(|c| (c.chapter.id.as_str(), c.chapter.role.as_str()))
        .collect()
}

/// The working draft of the slice, as a snapshot of it would hold it now.
fn draft_of(named: &OutlineResponse, scope: &str, reference: Option<&str>) -> Vec<Frozen> {
    let slice = Slice::of(scope, reference, order_of(named));
    named
        .chapters
        .iter()
        .flat_map(|c| {
            c.scenes
                .iter()
                .filter(|s| !s.excluded && slice.holds(&c.chapter.id, &s.id))
                .map(|s| Frozen {
                    chapter_id: c.chapter.id.clone(),
                    chapter_title: c.chapter.title.clone(),
                    scene_id: s.id.clone(),
                    scene_title: s.title.clone(),
                    content_hash: s.content_hash.clone(),
                })
        })
        .collect()
}

/// What a version holds of the slice, read by the version's own chapter order.
fn frozen_of(
    conn: &mut crate::db::Conn,
    version_id: &str,
    scope: &str,
    reference: Option<&str>,
) -> ApiResult<Vec<Frozen>> {
    let entries: Vec<VersionEntry> = version_entries::table
        .filter(version_entries::version_id.eq(version_id))
        .order((
            version_entries::chapter_position.asc(),
            version_entries::scene_position.asc(),
        ))
        .select(VersionEntry::as_select())
        .load(conn)?;
    let slice = Slice::of(
        scope,
        reference,
        slice::chapter_order(
            entries
                .iter()
                .map(|e| (e.chapter_id.as_str(), e.chapter_role.as_str())),
        ),
    );
    Ok(entries
        .into_iter()
        .filter(|e| slice.holds(&e.chapter_id, &e.scene_id))
        .map(|e| Frozen {
            chapter_id: e.chapter_id,
            chapter_title: e.chapter_title,
            scene_id: e.scene_id,
            scene_title: e.scene_title,
            content_hash: e.content_hash,
        })
        .collect())
}

/// The most recent snapshot that holds the slice, if it holds it exactly as
/// the draft has it now. Anything else means the draft has moved on, and a
/// new snapshot is due.
fn reusable_version(
    conn: &mut crate::db::Conn,
    work_id: &str,
    named: &OutlineResponse,
    scope: &str,
    reference: Option<&str>,
) -> ApiResult<Option<String>> {
    let draft = draft_of(named, scope, reference);
    let ids: Vec<String> = versions::table
        .filter(versions::work_id.eq(work_id))
        .order(versions::number.desc())
        .select(versions::id)
        .load(conn)?;
    for id in ids {
        let frozen = frozen_of(conn, &id, scope, reference)?;
        if frozen.is_empty() {
            continue;
        }
        return Ok((frozen == draft).then_some(id));
    }
    Ok(None)
}

/// The version a review should show: the latest snapshot if nothing has
/// changed, or a new one. Returns its id.
async fn current_version(
    state: &AppState,
    user: &AuthUser,
    work_id: &str,
    scope: &str,
    reference: Option<&str>,
) -> ApiResult<String> {
    let (wid, s, r) = (
        work_id.to_string(),
        scope.to_string(),
        reference.map(str::to_string),
    );
    let (reused, outline) = blocking(&state.pool, move |conn| {
        let named = named_outline(conn, &wid)?;
        if draft_of(&named, &s, r.as_deref()).is_empty() {
            return Err(ApiError::BadRequest(
                "there is nothing to review there yet: it has no text in the book".into(),
            ));
        }
        let reused = reusable_version(conn, &wid, &named, &s, r.as_deref())?;
        Ok((reused, works::load_outline(conn, &wid)?))
    })
    .await?;
    let version_id = match reused {
        Some(id) => id,
        None => {
            super::versions::snapshot(
                state,
                work_id,
                &user.id,
                outline,
                scope,
                reference,
                None,
                Some("Saved for review"),
            )
            .await?
            .version
            .id
        }
    };
    // Offered to readers, so no longer a private checkpoint. Only the
    // version's own state: what the book as a whole stands at is the owner's
    // to say, from the versions panel.
    let id = version_id.clone();
    blocking(&state.pool, move |conn| {
        let now = now_iso();
        diesel::update(
            versions::table
                .find(&id)
                .filter(versions::state.eq("draft")),
        )
        .set((
            versions::state.eq("published"),
            versions::published_at.eq(&now),
        ))
        .execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(version_id)
}

/// What the review is of, as the writer's book names it: "“The Door”", or the
/// book's own title for the whole of it.
fn title_of(named: &OutlineResponse, scope: &str, reference: Option<&str>) -> Option<String> {
    let title = match (scope, reference) {
        ("scene", Some(id)) => named
            .chapters
            .iter()
            .flat_map(|c| c.scenes.iter())
            .find(|s| s.id == id)
            .map(|s| s.title.clone()),
        (_, Some(id)) => named
            .chapters
            .iter()
            .find(|c| c.chapter.id == id)
            .map(|c| c.chapter.title.clone()),
        _ => Some(named.work.title.clone()),
    }?;
    Some(format!("“{}”", title.trim()))
}

/// For a notice: "“The Door” in “Gull Rock”", or "“Gull Rock”".
fn subject_of(named: &OutlineResponse, scope: &str, reference: Option<&str>) -> String {
    let book = format!("“{}”", named.work.title.trim());
    match (scope, title_of(named, scope, reference)) {
        ("work", _) => book,
        (_, Some(title)) => format!("{title} in {book}"),
        (_, None) => book,
    }
}

/// Refused unless the caller may put this slice up for review: an owner, or a
/// writer handed every chapter it touches.
fn require_reviewer(
    conn: &mut crate::db::Conn,
    work_id: &str,
    user: &AuthUser,
    named: &OutlineResponse,
    scope: &str,
    reference: Option<&str>,
) -> ApiResult<Work> {
    let (work, access) = support::work_access(conn, work_id, user)?;
    if !access.can(Ability::Write) {
        return Err(ApiError::Forbidden(
            "reviews are put up by the book's owners and writers".into(),
        ));
    }
    if let Some(mine) = &access.chapters {
        let slice = Slice::of(scope, reference, order_of(named));
        let touched: Vec<&str> = named
            .chapters
            .iter()
            .filter(|c| match &slice {
                Slice::Scene(id) => c.scenes.iter().any(|s| &s.id == id),
                other => other.holds_chapter(&c.chapter.id),
            })
            .map(|c| c.chapter.id.as_str())
            .collect();
        if scope == "work" || touched.is_empty() || touched.iter().any(|id| !mine.contains(*id)) {
            return Err(ApiError::Forbidden(
                "you can put up for review only the chapters you were handed".into(),
            ));
        }
    }
    Ok(work)
}

/// Refused unless `reference` names something of the right kind in the book.
fn check_reference(named: &OutlineResponse, scope: &str, reference: Option<&str>) -> ApiResult<()> {
    let found = match (scope, reference) {
        ("work", _) => true,
        ("scene", Some(id)) => named
            .chapters
            .iter()
            .any(|c| c.scenes.iter().any(|s| s.id == id)),
        ("chapter", Some(id)) => named.chapters.iter().any(|c| c.chapter.id == id),
        ("part", Some(id)) => named
            .chapters
            .iter()
            .any(|c| c.chapter.id == id && c.chapter.role == "part"),
        _ => false,
    };
    if found {
        Ok(())
    } else {
        Err(ApiError::BadRequest(format!(
            "that {scope} is not in this book"
        )))
    }
}

// --- what a review looks like -------------------------------------------------

/// Someone a review was shared with.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewPerson {
    /// Picked by name: who. Their address is never shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<AuthorCard>,
    /// Invited by address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Whether they have opened it.
    pub opened: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewView {
    pub id: String,
    pub work_id: String,
    #[schema(value_type = crate::domain::enums::PublicationScope)]
    pub scope: String,
    /// The part, chapter or scene; absent for the whole book.
    pub scope_ref_id: Option<String>,
    /// What it is of, as the book names it now: "“The Door”".
    pub title: String,
    /// The version readers are shown.
    pub version_id: String,
    pub version_label: String,
    /// Who it was offered to. A review put up from the outline is `platform`
    /// (everyone signed in) or `invited` (the people picked); one offered
    /// from the versions panel may be any visibility.
    #[schema(value_type = crate::domain::enums::Visibility)]
    pub visibility: String,
    pub prompt: Option<String>,
    pub closes_at: Option<String>,
    #[schema(value_type = crate::domain::enums::ReviewState)]
    pub state: String,
    /// The draft has moved on since the version readers are shown.
    pub outdated: bool,
    pub people: Vec<ReviewPerson>,
    /// Notes left on it, on any version.
    pub comment_count: i64,
    /// People who have opened it.
    pub reader_count: i64,
    pub created_at: String,
    pub updated_at: String,
    pub revoked_at: Option<String>,
}

fn state_of(publication: &Publication) -> &'static str {
    if publication.revoked_at.is_some() {
        ReviewState::Revoked.as_str()
    } else if publication
        .closes_at
        .as_deref()
        .is_some_and(|at| at < now_iso().as_str())
    {
        ReviewState::Closed.as_str()
    } else {
        ReviewState::Open.as_str()
    }
}

fn view_of(
    conn: &mut crate::db::Conn,
    publication: Publication,
    named: &OutlineResponse,
) -> ApiResult<ReviewView> {
    let scope = publication.scope.clone();
    let reference = publication.scope_ref_id.clone();
    let frozen = frozen_of(conn, &publication.version_id, &scope, reference.as_deref())?;
    let outdated = frozen != draft_of(named, &scope, reference.as_deref());
    let version_label: String = versions::table
        .find(&publication.version_id)
        .select(versions::label)
        .first(conn)?;
    let invites: Vec<PublicationInvite> = publication_invites::table
        .filter(publication_invites::publication_id.eq(&publication.id))
        .order(publication_invites::created_at.asc())
        .select(PublicationInvite::as_select())
        .load(conn)?;
    let picked: Vec<String> = invites.iter().filter_map(|i| i.user_id.clone()).collect();
    let cards = support::author_cards(conn, &picked)?;
    let people = invites
        .into_iter()
        .map(|invite| ReviewPerson {
            opened: invite.accepted_at.is_some(),
            user: invite
                .user_id
                .as_ref()
                .and_then(|id| cards.get(id).cloned()),
            email: invite.user_id.is_none().then_some(invite.email),
        })
        .collect();
    let comment_count: i64 = comments::table
        .filter(comments::publication_id.eq(&publication.id))
        .filter(comments::state.ne("deleted"))
        .count()
        .get_result(conn)?;
    let reader_count: i64 = reading_progress::table
        .filter(reading_progress::publication_id.eq(&publication.id))
        .count()
        .get_result(conn)?;
    Ok(ReviewView {
        title: title_of(named, &scope, reference.as_deref())
            .unwrap_or_else(|| "Something no longer in the book".into()),
        state: state_of(&publication).into(),
        id: publication.id,
        work_id: publication.work_id,
        scope,
        scope_ref_id: reference,
        version_id: publication.version_id,
        version_label,
        visibility: publication.visibility,
        prompt: publication.review_prompt,
        closes_at: publication.closes_at,
        outdated,
        people,
        comment_count,
        reader_count,
        created_at: publication.created_at,
        updated_at: publication.updated_at,
        revoked_at: publication.revoked_at,
    })
}

/// A review still open to its readers, for the badge on what it covers.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReviewBadge {
    pub id: String,
    #[schema(value_type = crate::domain::enums::PublicationScope)]
    pub scope: String,
    pub scope_ref_id: Option<String>,
    /// The draft has moved on since the version readers are shown.
    pub outdated: bool,
}

/// The book's open reviews, for its outline.
pub fn badges(
    conn: &mut crate::db::Conn,
    outline: &OutlineResponse,
) -> ApiResult<Vec<ReviewBadge>> {
    let rows: Vec<Publication> = publications::table
        .filter(publications::work_id.eq(&outline.work.id))
        .filter(publications::revoked_at.is_null())
        .order(publications::created_at.asc())
        .select(Publication::as_select())
        .load(conn)?;
    let open: Vec<Publication> = rows
        .into_iter()
        .filter(|p| state_of(p) == ReviewState::Open.as_str())
        .collect();
    if open.is_empty() {
        return Ok(Vec::new());
    }
    let named = works::with_placeholder_names(OutlineResponse {
        work: outline.work.clone(),
        chapters: outline
            .chapters
            .iter()
            .map(|c| works::OutlineChapter {
                chapter: c.chapter.clone(),
                scenes: c.scenes.clone(),
            })
            .collect(),
        access: None,
        place: None,
        statuses: None,
        reviews: None,
    });
    open.into_iter()
        .map(|p| {
            let frozen = frozen_of(conn, &p.version_id, &p.scope, p.scope_ref_id.as_deref())?;
            Ok(ReviewBadge {
                outdated: frozen != draft_of(&named, &p.scope, p.scope_ref_id.as_deref()),
                id: p.id,
                scope: p.scope,
                scope_ref_id: p.scope_ref_id,
            })
        })
        .collect()
}

// --- who it is shared with ---------------------------------------------------

/// Someone newly given a review, to be told about it.
enum Newcomer {
    Account(String),
    Address(String),
}

/// Adds the people picked and the addresses typed, and with `replace` takes
/// away anyone else. Returns who was not on it before.
fn share_with(
    conn: &mut crate::db::Conn,
    publication_id: &str,
    inviter: &AuthUser,
    user_ids: &[String],
    emails: &[String],
    replace: bool,
) -> ApiResult<Vec<Newcomer>> {
    let hidden: HashSet<String> = crate::domain::restrictions::hidden_users(conn)?
        .into_iter()
        .collect();
    let accounts: Vec<(String, String)> = users::table
        .filter(users::id.eq_any(user_ids))
        .select((users::id, users::email))
        .load(conn)?;
    let accounts: Vec<(String, String)> = accounts
        .into_iter()
        .filter(|(id, _)| !hidden.contains(id) && id != &inviter.id)
        .map(|(id, email)| (id, email.trim().to_lowercase()))
        .collect();
    if accounts.len() < user_ids.iter().filter(|id| **id != inviter.id).count() {
        return Err(ApiError::BadRequest(
            "one of the people picked is not on Just Type Studio".into(),
        ));
    }

    let existing: Vec<PublicationInvite> = publication_invites::table
        .filter(publication_invites::publication_id.eq(publication_id))
        .select(PublicationInvite::as_select())
        .load(conn)?;
    if replace {
        let keep_ids: HashSet<&str> = accounts.iter().map(|(id, _)| id.as_str()).collect();
        let keep_emails: HashSet<&str> = emails.iter().map(String::as_str).collect();
        let gone: Vec<&str> = existing
            .iter()
            .filter(|i| match &i.user_id {
                Some(id) => {
                    !keep_ids.contains(id.as_str()) && !keep_emails.contains(i.email.as_str())
                }
                None => !keep_emails.contains(i.email.as_str()),
            })
            .map(|i| i.id.as_str())
            .collect();
        diesel::delete(publication_invites::table.filter(publication_invites::id.eq_any(gone)))
            .execute(conn)?;
    }

    let now = now_iso();
    let mut newcomers = Vec::new();
    for (user_id, email) in &accounts {
        let was = existing.iter().find(|i| &i.email == email);
        match was {
            // Typed as an address before, picked by name now: the same person.
            Some(invite) => {
                if invite.user_id.is_none() {
                    diesel::update(publication_invites::table.find(&invite.id))
                        .set(publication_invites::user_id.eq(user_id))
                        .execute(conn)?;
                }
            }
            None => {
                diesel::insert_into(publication_invites::table)
                    .values(&NewPublicationInvite {
                        id: new_id(),
                        publication_id: publication_id.into(),
                        email: email.clone(),
                        invited_by: inviter.id.clone(),
                        accepted_at: None,
                        accepted_by: None,
                        created_at: now.clone(),
                        user_id: Some(user_id.clone()),
                    })
                    .execute(conn)?;
                newcomers.push(Newcomer::Account(user_id.clone()));
            }
        }
    }
    let picked: HashSet<&str> = accounts.iter().map(|(_, e)| e.as_str()).collect();
    for email in emails {
        if picked.contains(email.as_str()) || existing.iter().any(|i| &i.email == email) {
            continue;
        }
        diesel::insert_into(publication_invites::table)
            .values(&NewPublicationInvite {
                id: new_id(),
                publication_id: publication_id.into(),
                email: email.clone(),
                invited_by: inviter.id.clone(),
                accepted_at: None,
                accepted_by: None,
                created_at: now.clone(),
                user_id: None,
            })
            .execute(conn)?;
        newcomers.push(Newcomer::Address(email.clone()));
    }
    Ok(newcomers)
}

/// The invitation sent to an address. Fixed words, as every mail the site
/// sends on someone's behalf is: they choose who and what, never the message.
pub fn review_invite_message(
    public_url: &str,
    to: &str,
    inviter: &str,
    subject: &str,
    publication_id: &str,
) -> Message {
    let link = format!("{public_url}/review/{publication_id}");
    let body = format!(
        "Hello,\n\n\
{inviter} would like your thoughts on {subject}, on Just Type Studio.\n\n\
Read it and leave notes:\n{link}\n\n\
You will need to sign in, or make an account, with this address. You are \
getting this because {inviter} typed it; we will not write to you about it again.\n"
    );
    let button = "display:inline-block;padding:10px 16px;background:#7a4f2a;color:#fff;text-decoration:none;border-radius:4px";
    let html = format!(
        "<!doctype html><html><body style=\"font-family:Georgia,serif;color:#222;max-width:560px;margin:0 auto;padding:24px;line-height:1.5\">\
<p>Hello,</p>\
<p>{inviter} would like your thoughts on {subject}, on Just Type Studio.</p>\
<p><a href=\"{link}\" style=\"{button}\">Read it and leave notes</a></p>\
<p style=\"color:#666;font-size:13px\">You will need to sign in, or make an account, with this address. \
You are getting this because {inviter} typed it; we will not write to you about it again.</p>\
</body></html>",
        inviter = crate::jobs::escape(inviter),
        subject = crate::jobs::escape(subject),
        link = crate::jobs::escape(&link),
    );
    Message {
        to: to.into(),
        subject: format!("{inviter} would like your thoughts on {subject}"),
        body,
        html: Some(html),
        headers: Vec::new(),
    }
}

/// Tells the people just given a review: an inbox item for an account, and
/// for an address a mail, plus an inbox item if it belongs to someone.
fn tell_newcomers(
    conn: &mut crate::db::Conn,
    public_url: &str,
    publication: &Publication,
    actor: &AuthUser,
    subject: &str,
    newcomers: &[Newcomer],
) -> ApiResult<()> {
    if newcomers.is_empty() {
        return Ok(());
    }
    let inviter: String = users::table
        .find(&actor.id)
        .select(users::display_name)
        .first(conn)?;
    let detail = json!({ "subject": subject, "publicationId": publication.id }).to_string();
    let notify = |conn: &mut crate::db::Conn, user_id: &str| {
        super::inbox::notify(
            conn,
            user_id,
            "review.invited",
            ("publication", &publication.id),
            Some(("work", &publication.work_id)),
            Some(&actor.id),
            &detail,
        )
    };
    for newcomer in newcomers {
        match newcomer {
            Newcomer::Account(user_id) => notify(conn, user_id)?,
            Newcomer::Address(email) => {
                let owner: Option<String> = users::table
                    .filter(users::email.eq(email))
                    .filter(users::email_verified_at.is_not_null())
                    .select(users::id)
                    .first(conn)
                    .optional()?;
                if let Some(user_id) = owner {
                    notify(conn, &user_id)?;
                }
                crate::mail::enqueue(
                    conn,
                    &review_invite_message(public_url, email, &inviter, subject, &publication.id),
                    crate::clock::now() + Duration::days(GIVE_UP_DAYS),
                )?;
            }
        }
    }
    Ok(())
}

/// Everyone who has taken part in a review: the people it was shared with,
/// and anyone who has opened it or left a note on it.
fn participants(conn: &mut crate::db::Conn, publication_id: &str) -> ApiResult<Vec<String>> {
    let mut ids: HashSet<String> = HashSet::new();
    let invites: Vec<(Option<String>, String)> = publication_invites::table
        .filter(publication_invites::publication_id.eq(publication_id))
        .select((publication_invites::user_id, publication_invites::email))
        .load(conn)?;
    let mut addresses = Vec::new();
    for (user_id, email) in invites {
        match user_id {
            Some(id) => {
                ids.insert(id);
            }
            None => addresses.push(email),
        }
    }
    ids.extend(
        users::table
            .filter(users::email.eq_any(&addresses))
            .filter(users::email_verified_at.is_not_null())
            .select(users::id)
            .load::<String>(conn)?,
    );
    ids.extend(
        reading_progress::table
            .filter(reading_progress::publication_id.eq(publication_id))
            .select(reading_progress::user_id)
            .load::<String>(conn)?,
    );
    ids.extend(
        comments::table
            .filter(comments::publication_id.eq(publication_id))
            .select(comments::author_id)
            .load::<String>(conn)?,
    );
    let mut ids: Vec<String> = ids.into_iter().collect();
    ids.sort();
    Ok(ids)
}

// --- requests -------------------------------------------------------------

fn checked_people(emails: &[String]) -> ApiResult<Vec<String>> {
    let mut out = Vec::with_capacity(emails.len());
    for email in emails {
        out.push(super::readers::normalise(email)?);
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// `""` clears it; otherwise a date and time the review closes at.
fn checked_closes_at(value: &str) -> ApiResult<Option<String>> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let at = chrono::DateTime::parse_from_rfc3339(value).map_err(|_| {
        ApiError::BadRequest("closesAt is a date and time, like 2026-11-01T00:00:00Z".into())
    })?;
    Ok(Some(
        at.with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    ))
}

fn visibility_for(audience: &str) -> ApiResult<&'static str> {
    match audience {
        "everyone" => Ok("platform"),
        "selected" => Ok("invited"),
        _ => Err(ApiError::BadRequest(format!(
            "audience must be one of {}",
            ReviewAudience::options()
        ))),
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateReviewRequest {
    #[schema(value_type = crate::domain::enums::PublicationScope)]
    pub scope: String,
    /// The part, chapter or scene; left out for the whole book.
    #[serde(default)]
    pub scope_ref_id: Option<String>,
    #[schema(value_type = crate::domain::enums::ReviewAudience)]
    pub audience: String,
    /// For `selected`: people picked by name (`GET /api/people`).
    #[serde(default)]
    pub user_ids: Vec<String>,
    /// For `selected`: addresses, for people who may have no account yet.
    #[serde(default)]
    pub emails: Vec<String>,
    /// What the writer wants feedback on.
    #[serde(default)]
    pub prompt: Option<String>,
    /// When it closes, if it should.
    #[serde(default)]
    pub closes_at: Option<String>,
}

/// Puts a scene, chapter, part or the whole book up for review.
#[utoipa::path(
    post,
    path = "/api/works/{work_id}/reviews",
    tag = "reviews",
    params(("work_id" = String, Path)),
    request_body = CreateReviewRequest,
    responses(
        (status = 200, description = "The review", body = ReviewView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
    Json(body): Json<CreateReviewRequest>,
) -> ApiResult<Json<ReviewView>> {
    if !PublicationScope::accepts(&body.scope) {
        return Err(ApiError::BadRequest(format!(
            "scope must be one of {}",
            PublicationScope::options()
        )));
    }
    let reference = match body.scope.as_str() {
        "work" => None,
        _ => Some(body.scope_ref_id.clone().ok_or_else(|| {
            ApiError::BadRequest(format!("scopeRefId names the {} to review", body.scope))
        })?),
    };
    let visibility = visibility_for(&body.audience)?;
    let emails = checked_people(&body.emails)?;
    if visibility == "invited" && emails.is_empty() && body.user_ids.is_empty() {
        return Err(ApiError::BadRequest(
            "pick at least one person to review it, or offer it to everyone".into(),
        ));
    }
    let closes_at = match &body.closes_at {
        Some(at) => checked_closes_at(at)?,
        None => None,
    };
    let prompt = body
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);

    // Who may, before anything is frozen.
    let (wid, s, r, u) = (
        work_id.clone(),
        body.scope.clone(),
        reference.clone(),
        user.clone(),
    );
    blocking(&state.pool, move |conn| {
        let named = named_outline(conn, &wid)?;
        check_reference(&named, &s, r.as_deref())?;
        require_reviewer(conn, &wid, &u, &named, &s, r.as_deref())?;
        Ok(())
    })
    .await?;

    let screen =
        super::publications::screen_publication(&state, &user, None, None, prompt.as_deref())
            .await?;
    let version_id =
        current_version(&state, &user, &work_id, &body.scope, reference.as_deref()).await?;

    let public_url = state.config.public_url.clone();
    let view = blocking(&state.pool, move |conn| {
        let named = named_outline(conn, &work_id)?;
        let work = require_reviewer(
            conn,
            &work_id,
            &user,
            &named,
            &body.scope,
            reference.as_deref(),
        )?;
        let now = now_iso();
        let row = NewPublication {
            id: new_id(),
            work_id: work.id.clone(),
            version_id: version_id.clone(),
            visibility: visibility.into(),
            scope: body.scope.clone(),
            scope_ref_id: reference.clone(),
            review_prompt: prompt,
            opens_at: None,
            closes_at,
            created_at: now.clone(),
            updated_at: now,
            max_reviewers: None,
            spoiler_safe: i32::from(work.kind == "serial"),
            link_secret: Some(crate::auth::generate_link_token()),
        };
        let subject = subject_of(&named, &body.scope, reference.as_deref());
        let newcomers = conn.transaction::<_, ApiError, _>(|conn| {
            diesel::insert_into(publications::table)
                .values(&row)
                .execute(conn)?;
            let provider = moderation::provider_name(conn)?;
            moderation::record(conn, &screen, "publication", &row.id, &user.id, &provider)?;
            let newcomers = if visibility == "invited" {
                share_with(conn, &row.id, &user, &body.user_ids, &emails, false)?
            } else {
                Vec::new()
            };
            super::audit::record(
                conn,
                Some(&user.id),
                Some(&work.id),
                AuditAction::PublicationCreated,
                &format!("{subject} put up for review, {visibility}"),
            )?;
            super::collaborators::record(
                conn,
                &work.id,
                &user.id,
                ActivityKind::PublicationOpened,
                Some(&row.id),
                &format!("{subject} for review"),
            )?;
            Ok(newcomers)
        })?;
        let publication = support::find_publication(conn, &row.id)?;
        tell_newcomers(conn, &public_url, &publication, &user, &subject, &newcomers)?;
        view_of(conn, publication, &named)
    })
    .await?;
    Ok(Json(view))
}

/// Every review of the book, open or not, newest first.
#[utoipa::path(
    get,
    path = "/api/works/{work_id}/reviews",
    tag = "reviews",
    params(("work_id" = String, Path)),
    responses(
        (status = 200, description = "The book's reviews", body = Vec<ReviewView>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(work_id): Path<String>,
) -> ApiResult<Json<Vec<ReviewView>>> {
    let views = blocking(&state.pool, move |conn| {
        support::work_for(conn, &work_id, &user, Ability::Read)?;
        let named = named_outline(conn, &work_id)?;
        let rows: Vec<Publication> = publications::table
            .filter(publications::work_id.eq(&work_id))
            .order(publications::created_at.desc())
            .select(Publication::as_select())
            .load(conn)?;
        rows.into_iter().map(|p| view_of(conn, p, &named)).collect()
    })
    .await?;
    Ok(Json(views))
}

/// A review the caller may manage, with the book as it stands.
fn review_for(
    conn: &mut crate::db::Conn,
    review_id: &str,
    user: &AuthUser,
) -> ApiResult<(Publication, Work, OutlineResponse)> {
    let publication = support::find_publication(conn, review_id)?;
    let named = named_outline(conn, &publication.work_id)?;
    let work = require_reviewer(
        conn,
        &publication.work_id,
        user,
        &named,
        &publication.scope,
        publication.scope_ref_id.as_deref(),
    )?;
    Ok((publication, work, named))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReviewRequest {
    #[serde(default)]
    #[schema(value_type = Option<crate::domain::enums::ReviewAudience>)]
    pub audience: Option<String>,
    /// The people picked, in full: anyone left out is taken off it. Sent
    /// with `emails`, or neither.
    #[serde(default)]
    pub user_ids: Option<Vec<String>>,
    #[serde(default)]
    pub emails: Option<Vec<String>>,
    /// An empty string clears it.
    #[serde(default)]
    pub prompt: Option<String>,
    /// An empty string clears it.
    #[serde(default)]
    pub closes_at: Option<String>,
}

/// Changes who a review is for, its prompt, or when it closes. Anyone newly
/// given it is told.
#[utoipa::path(
    patch,
    path = "/api/reviews/{review_id}",
    tag = "reviews",
    params(("review_id" = String, Path)),
    request_body = UpdateReviewRequest,
    responses(
        (status = 200, description = "The review", body = ReviewView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(review_id): Path<String>,
    Json(body): Json<UpdateReviewRequest>,
) -> ApiResult<Json<ReviewView>> {
    let visibility = body.audience.as_deref().map(visibility_for).transpose()?;
    let emails = body.emails.as_deref().map(checked_people).transpose()?;
    let closes_at = body
        .closes_at
        .as_deref()
        .map(checked_closes_at)
        .transpose()?;
    let screen = match &body.prompt {
        Some(prompt) if !prompt.trim().is_empty() => Some(
            super::publications::screen_publication(&state, &user, None, None, Some(prompt))
                .await?,
        ),
        _ => None,
    };
    let public_url = state.config.public_url.clone();
    let view = blocking(&state.pool, move |conn| {
        let (publication, work, named) = review_for(conn, &review_id, &user)?;
        let subject = subject_of(
            &named,
            &publication.scope,
            publication.scope_ref_id.as_deref(),
        );
        let visibility = visibility.unwrap_or(match publication.visibility.as_str() {
            "invited" => "invited",
            "platform" => "platform",
            _ => "",
        });
        let newcomers = conn.transaction::<_, ApiError, _>(|conn| {
            let now = now_iso();
            if !visibility.is_empty() && visibility != publication.visibility {
                diesel::update(publications::table.find(&publication.id))
                    .set(publications::visibility.eq(visibility))
                    .execute(conn)?;
            }
            if let Some(prompt) = &body.prompt {
                let prompt = prompt.trim();
                diesel::update(publications::table.find(&publication.id))
                    .set(publications::review_prompt.eq((!prompt.is_empty()).then_some(prompt)))
                    .execute(conn)?;
                if let Some(screen) = &screen {
                    let provider = moderation::provider_name(conn)?;
                    moderation::record(
                        conn,
                        screen,
                        "publication",
                        &publication.id,
                        &user.id,
                        &provider,
                    )?;
                }
            }
            if let Some(closes_at) = &closes_at {
                diesel::update(publications::table.find(&publication.id))
                    .set(publications::closes_at.eq(closes_at))
                    .execute(conn)?;
            }
            let newcomers = match (&body.user_ids, &emails) {
                (None, None) => Vec::new(),
                (ids, emails) => share_with(
                    conn,
                    &publication.id,
                    &user,
                    ids.as_deref().unwrap_or(&[]),
                    emails.as_deref().unwrap_or(&[]),
                    true,
                )?,
            };
            let left: i64 = publication_invites::table
                .filter(publication_invites::publication_id.eq(&publication.id))
                .count()
                .get_result(conn)?;
            if visibility == "invited" && left == 0 {
                return Err(ApiError::BadRequest(
                    "pick at least one person to review it, or offer it to everyone".into(),
                ));
            }
            diesel::update(publications::table.find(&publication.id))
                .set(publications::updated_at.eq(&now))
                .execute(conn)?;
            super::audit::record(
                conn,
                Some(&user.id),
                Some(&work.id),
                AuditAction::PublicationChanged,
                &format!("{}: the review of {subject}", publication.id),
            )?;
            Ok(newcomers)
        })?;
        let publication = support::find_publication(conn, &publication.id)?;
        // Only an open review is worth telling anyone about.
        if state_of(&publication) == ReviewState::Open.as_str() {
            tell_newcomers(conn, &public_url, &publication, &user, &subject, &newcomers)?;
        }
        view_of(conn, publication, &named)
    })
    .await?;
    Ok(Json(view))
}

/// Points a review at the draft as it is now: the latest snapshot if nothing
/// has changed since, or a new one. Its comments stay, each marked with the
/// version it was written on, and everyone who has taken part is told.
#[utoipa::path(
    post,
    path = "/api/reviews/{review_id}/update",
    tag = "reviews",
    params(("review_id" = String, Path)),
    responses(
        (status = 200, description = "The review, on the latest version", body = ReviewView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn bring_up_to_date(
    State(state): State<AppState>,
    user: AuthUser,
    Path(review_id): Path<String>,
) -> ApiResult<Json<ReviewView>> {
    let (rid, u) = (review_id.clone(), user.clone());
    let publication = blocking(&state.pool, move |conn| Ok(review_for(conn, &rid, &u)?.0)).await?;
    let version_id = current_version(
        &state,
        &user,
        &publication.work_id,
        &publication.scope,
        publication.scope_ref_id.as_deref(),
    )
    .await?;
    let view = blocking(&state.pool, move |conn| {
        let (publication, work, named) = review_for(conn, &review_id, &user)?;
        if publication.version_id != version_id {
            let label: String = versions::table
                .find(&version_id)
                .select(versions::label)
                .first(conn)?;
            let subject = subject_of(
                &named,
                &publication.scope,
                publication.scope_ref_id.as_deref(),
            );
            conn.transaction::<_, ApiError, _>(|conn| {
                diesel::update(publications::table.find(&publication.id))
                    .set((
                        publications::version_id.eq(&version_id),
                        publications::updated_at.eq(now_iso()),
                    ))
                    .execute(conn)?;
                super::audit::record(
                    conn,
                    Some(&user.id),
                    Some(&work.id),
                    AuditAction::PublicationChanged,
                    &format!("{}: brought up to date, {label}", publication.id),
                )?;
                let detail =
                    json!({ "subject": subject, "publicationId": publication.id }).to_string();
                for reader in participants(conn, &publication.id)? {
                    super::inbox::notify(
                        conn,
                        &reader,
                        "review.updated",
                        ("publication", &publication.id),
                        Some(("work", &publication.work_id)),
                        Some(&user.id),
                        &detail,
                    )?;
                }
                Ok(())
            })?;
        }
        let publication = support::find_publication(conn, &publication.id)?;
        view_of(conn, publication, &named)
    })
    .await?;
    Ok(Json(view))
}

/// Closes a review to its readers. Every comment stays, for the writers.
#[utoipa::path(
    post,
    path = "/api/reviews/{review_id}/revoke",
    tag = "reviews",
    params(("review_id" = String, Path)),
    responses(
        (status = 200, description = "The review, revoked", body = ReviewView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn revoke(
    State(state): State<AppState>,
    user: AuthUser,
    Path(review_id): Path<String>,
) -> ApiResult<Json<ReviewView>> {
    let view = blocking(&state.pool, move |conn| {
        let (publication, work, named) = review_for(conn, &review_id, &user)?;
        if publication.revoked_at.is_none() {
            let now = now_iso();
            diesel::update(publications::table.find(&publication.id))
                .set((
                    publications::revoked_at.eq(&now),
                    publications::updated_at.eq(&now),
                ))
                .execute(conn)?;
            super::audit::record(
                conn,
                Some(&user.id),
                Some(&work.id),
                AuditAction::PublicationChanged,
                &format!("{}: revoked, comments kept", publication.id),
            )?;
        }
        let publication = support::find_publication(conn, &publication.id)?;
        view_of(conn, publication, &named)
    })
    .await?;
    Ok(Json(view))
}

/// Opens a revoked or closed review again, to the same readers. A closing
/// date that has passed is cleared; set a new one to close it again.
#[utoipa::path(
    post,
    path = "/api/reviews/{review_id}/reopen",
    tag = "reviews",
    params(("review_id" = String, Path)),
    responses(
        (status = 200, description = "The review, open again", body = ReviewView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn reopen(
    State(state): State<AppState>,
    user: AuthUser,
    Path(review_id): Path<String>,
) -> ApiResult<Json<ReviewView>> {
    let view = blocking(&state.pool, move |conn| {
        let (publication, work, named) = review_for(conn, &review_id, &user)?;
        let now = now_iso();
        let past = publication
            .closes_at
            .as_deref()
            .is_some_and(|at| at < now.as_str());
        diesel::update(publications::table.find(&publication.id))
            .set((
                publications::revoked_at.eq(None::<String>),
                publications::updated_at.eq(&now),
            ))
            .execute(conn)?;
        if past {
            diesel::update(publications::table.find(&publication.id))
                .set(publications::closes_at.eq(None::<String>))
                .execute(conn)?;
        }
        super::audit::record(
            conn,
            Some(&user.id),
            Some(&work.id),
            AuditAction::PublicationChanged,
            &format!("{}: opened again", publication.id),
        )?;
        let publication = support::find_publication(conn, &publication.id)?;
        view_of(conn, publication, &named)
    })
    .await?;
    Ok(Json(view))
}

// --- finding people ---------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct PeopleQuery {
    /// Part of a display name, two characters or more.
    pub q: String,
}

/// People on the site by name, to pick as reviewers. Names only: an address
/// is never given out.
#[utoipa::path(
    get,
    path = "/api/people",
    tag = "reviews",
    params(PeopleQuery),
    responses(
        (status = 200, description = "Up to eight people whose names match", body = Vec<AuthorCard>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn people(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<PeopleQuery>,
) -> ApiResult<Json<Vec<AuthorCard>>> {
    let q = query.q.trim().to_lowercase();
    if q.chars().count() < 2 {
        return Ok(Json(Vec::new()));
    }
    let found = blocking(&state.pool, move |conn| {
        let hidden = crate::domain::restrictions::hidden_users(conn)?;
        let pattern = format!(
            "%{}%",
            q.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let rows: Vec<(String, String)> = users::table
            .filter(users::display_name.like(pattern).escape('\\'))
            .filter(users::id.ne(&user.id))
            .filter(users::id.ne_all(hidden))
            .order(users::display_name.asc())
            .limit(8)
            .select((users::id, users::display_name))
            .load(conn)?;
        Ok(rows
            .into_iter()
            .map(|(id, display_name)| AuthorCard { id, display_name })
            .collect::<Vec<_>>())
    })
    .await?;
    Ok(Json(found))
}
