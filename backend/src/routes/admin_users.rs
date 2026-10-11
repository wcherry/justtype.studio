//! Accounts, for site staff: find one, see it whole, and fix what is wrong
//! with it. Create, edit, send a reset, sign out every device, deactivate,
//! delete; internal notes; making and unmaking moderators.
//!
//! Every route takes a [`StaffUser`] and asks for the one [`Permission`] it
//! needs. A moderator sees names, activity counts and restrictions, and never
//! an address, a legal name, a phone number, an IP or a session. Staff never
//! see or set a password and never see a reset or setup token; they can only
//! cause the normal email to be sent.
use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::auth::{self, Permission, StaffRole, StaffUser};
use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::enums::AuditAction;
use crate::domain::new_id;
use crate::domain::restrictions as rules;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::mail::Message;
use crate::state::AppState;

use super::account::{mint, revoke_sessions};
use super::audit::record_staff;
use super::auth::normalize_email;
use super::support::{self, AuthorCard};

/// A setup link lasts a week; a reset lasts an hour.
const SETUP_DAYS: i64 = 7;
const RESET_MINUTES: i64 = 60;
/// Resets staff may send to one account in an hour.
const RESETS_PER_HOUR: i64 = 3;
const PAGE: usize = 50;

fn clean_reason(reason: &Option<String>) -> ApiResult<String> {
    let reason = reason.as_deref().unwrap_or("").trim();
    if reason.is_empty() {
        return Err(ApiError::BadRequest("a reason is required".into()));
    }
    if reason.chars().count() > 1_000 {
        return Err(ApiError::BadRequest(
            "keep the reason under 1,000 characters".into(),
        ));
    }
    Ok(reason.to_string())
}

/// `(is_admin, is_moderator)` of an account.
fn staff_flags(conn: &mut Conn, user_id: &str) -> ApiResult<(bool, bool)> {
    users::table
        .find(user_id)
        .select((users::is_admin, users::is_moderator))
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("no such account".into()))
}

/// Staff can't act on themselves or on each other, except an admin on a
/// moderator. Admins are managed from the command line.
pub fn guard_target(conn: &mut Conn, actor: &StaffUser, target_id: &str) -> ApiResult<()> {
    if actor.id() == target_id {
        return Err(ApiError::Forbidden(
            "you can't do that to your own account".into(),
        ));
    }
    let (admin, moderator) = staff_flags(conn, target_id)?;
    if admin {
        return Err(ApiError::Forbidden(
            "administrators are managed from the command line".into(),
        ));
    }
    if moderator && !actor.is_admin() {
        return Err(ApiError::Forbidden(
            "only an administrator can act on a moderator".into(),
        ));
    }
    Ok(())
}

pub fn role_of(admin: bool, moderator: bool) -> Option<StaffRole> {
    if admin {
        Some(StaffRole::Admin)
    } else if moderator {
        Some(StaffRole::Moderator)
    } else {
        None
    }
}

/// `active`, `unverified`, `restricted`, `banned` or `deactivated`, and when
/// the restriction that decided it ends.
fn status_of(
    active: &[Restriction],
    deactivated: bool,
    verified: bool,
) -> (&'static str, Option<String>) {
    if deactivated {
        return ("deactivated", None);
    }
    if let Some(ban) = rules::site_ban(active) {
        return ("banned", ban.ends_at.clone());
    }
    if let Some(first) = active.first() {
        return ("restricted", first.ends_at.clone());
    }
    if !verified {
        return ("unverified", None);
    }
    ("active", None)
}

// --- the landing page --------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub role: StaffRole,
    pub queue_open: i64,
    pub queue_held: i64,
    pub appeals_open: i64,
    /// Restrictions ending in the next 24 hours.
    pub ending_soon: i64,
    pub signups_today: i64,
    pub reports_today: i64,
    /// Closing accounts whose deletion is waiting on an open appeal.
    pub waiting_on_appeal: i64,
}

#[utoipa::path(
    get,
    path = "/api/admin/overview",
    tag = "admin",
    responses(
        (status = 200, description = "Counts for the landing page", body = Overview),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn overview(
    State(state): State<AppState>,
    staff: StaffUser,
) -> ApiResult<Json<Overview>> {
    staff.require(Permission::ViewQueue)?;
    let role = staff.role;
    let out = blocking(&state.pool, move |conn| {
        let now = clock::now();
        let now_s = clock::now_iso();
        let day_start = clock::day_start(clock::prompt_day(now))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let tomorrow = (now + chrono::Duration::hours(24))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let count_items = |conn: &mut Conn, state: &str| -> ApiResult<i64> {
            Ok(moderation_items::table
                .filter(moderation_items::state.eq(state))
                .count()
                .get_result(conn)?)
        };
        let reports_today = reports::table
            .filter(reports::created_at.ge(&day_start))
            .count()
            .get_result::<i64>(conn)?
            + comment_reports::table
                .filter(comment_reports::created_at.ge(&day_start))
                .count()
                .get_result::<i64>(conn)?
            + prompt_critique_reports::table
                .filter(prompt_critique_reports::created_at.ge(&day_start))
                .count()
                .get_result::<i64>(conn)?;
        Ok(Overview {
            role,
            queue_open: count_items(conn, "open")?,
            queue_held: count_items(conn, "held")?,
            appeals_open: appeals::table
                .filter(appeals::state.eq("open"))
                .count()
                .get_result(conn)?,
            ending_soon: restrictions::table
                .filter(restrictions::lifted_at.is_null())
                .filter(restrictions::kind.ne("warning"))
                .filter(restrictions::ends_at.gt(&now_s))
                .filter(restrictions::ends_at.le(&tomorrow))
                .count()
                .get_result(conn)?,
            signups_today: users::table
                .filter(users::created_at.ge(&day_start))
                .count()
                .get_result(conn)?,
            reports_today,
            waiting_on_appeal: waiting_on_appeal(conn)?,
        })
    })
    .await?;
    Ok(Json(out))
}

/// Accounts whose deletion is held by an open appeal on a closing restriction.
pub fn waiting_on_appeal(conn: &mut Conn) -> ApiResult<i64> {
    let now = clock::now_iso();
    Ok(users::table
        .filter(users::purge_at.is_not_null())
        .filter(users::purge_at.le(&now))
        .filter(
            users::id.eq_any(
                restrictions::table
                    .inner_join(appeals::table)
                    .filter(appeals::state.eq("open"))
                    .select(restrictions::user_id),
            ),
        )
        .count()
        .get_result(conn)?)
}

// --- the list ------------------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct UserListQuery {
    /// Part of a display name; for administrators also a whole email or an id.
    #[serde(default)]
    pub q: Option<String>,
    /// `active`, `unverified`, `restricted`, `banned` or `deactivated`.
    #[serde(default)]
    pub status: Option<String>,
    /// `moderator`, `admin` or `none`.
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub verified: Option<bool>,
    /// `joined` (default), `last_seen` or `reports`.
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserRow {
    pub id: String,
    pub display_name: String,
    /// Administrators only.
    pub email: Option<String>,
    pub created_at: String,
    pub last_seen_at: Option<String>,
    pub email_verified: bool,
    pub two_factor: bool,
    pub role: Option<StaffRole>,
    pub status: String,
    /// When the restriction behind the status ends. Absent when permanent.
    pub status_ends: Option<String>,
    pub open_reports: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserList {
    pub users: Vec<UserRow>,
    pub next_cursor: Option<String>,
}

type Candidate = (
    String,         // id
    String,         // display_name
    String,         // email
    String,         // created_at
    Option<String>, // email_verified_at
    Option<String>, // totp_enabled_at
    bool,           // is_admin
    bool,           // is_moderator
    Option<String>, // deactivated_at
);

fn like_escape(q: &str) -> String {
    q.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[utoipa::path(
    get,
    path = "/api/admin/users",
    tag = "admin",
    params(UserListQuery),
    responses(
        (status = 200, description = "A page of accounts", body = UserList),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_users(
    State(state): State<AppState>,
    staff: StaffUser,
    Query(query): Query<UserListQuery>,
) -> ApiResult<Json<UserList>> {
    staff.require(Permission::ViewQueue)?;
    let pii = staff.can(Permission::ViewAccountPii);
    let sort = query.sort.clone().unwrap_or_else(|| "joined".into());
    if !["joined", "last_seen", "reports"].contains(&sort.as_str()) {
        return Err(ApiError::BadRequest(
            "sort must be joined, last_seen or reports".into(),
        ));
    }
    if let Some(status) = query.status.as_deref() {
        if ![
            "active",
            "unverified",
            "restricted",
            "banned",
            "deactivated",
        ]
        .contains(&status)
        {
            return Err(ApiError::BadRequest("unknown status".into()));
        }
    }
    let page = blocking(&state.pool, move |conn| {
        let mut q = users::table
            .filter(users::id.ne(super::privacy::FORMER_MEMBER))
            .into_boxed();
        if let Some(text) = query.q.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            let like = format!("%{}%", like_escape(text));
            q = if pii {
                q.filter(
                    users::display_name
                        .like(like.clone())
                        .escape('\\')
                        .or(users::email.like(like).escape('\\'))
                        .or(users::id.eq(text.to_string())),
                )
            } else {
                // A moderator searches names only, and an address typed in
                // finds nothing, whether or not it exists.
                q.filter(users::display_name.like(like).escape('\\'))
            };
        }
        match query.role.as_deref() {
            Some("admin") => q = q.filter(users::is_admin.eq(true)),
            Some("moderator") => q = q.filter(users::is_moderator.eq(true)),
            Some("none") => {
                q = q.filter(users::is_admin.eq(false).and(users::is_moderator.eq(false)))
            }
            _ => {}
        }
        match query.verified {
            Some(true) => q = q.filter(users::email_verified_at.is_not_null()),
            Some(false) => q = q.filter(users::email_verified_at.is_null()),
            None => {}
        }
        let rows: Vec<Candidate> = q
            .select((
                users::id,
                users::display_name,
                users::email,
                users::created_at,
                users::email_verified_at,
                users::totp_enabled_at,
                users::is_admin,
                users::is_moderator,
                users::deactivated_at,
            ))
            .load(conn)?;

        // Restrictions in force, for every account in one query.
        let now = clock::now_iso();
        let in_force: Vec<Restriction> = restrictions::table
            .filter(restrictions::kind.ne("warning"))
            .filter(restrictions::lifted_at.is_null())
            .filter(restrictions::starts_at.le(&now))
            .filter(
                restrictions::ends_at
                    .is_null()
                    .or(restrictions::ends_at.gt(&now)),
            )
            .select(Restriction::as_select())
            .load(conn)?;
        let mut by_user: HashMap<String, Vec<Restriction>> = HashMap::new();
        for r in in_force {
            by_user.entry(r.user_id.clone()).or_default().push(r);
        }
        let last_seen: HashMap<String, Option<String>> = sessions::table
            .group_by(sessions::user_id)
            .select((sessions::user_id, diesel::dsl::max(sessions::last_seen_at)))
            .load::<(String, Option<String>)>(conn)?
            .into_iter()
            .collect();
        let open: HashMap<String, i64> = moderation_items::table
            .filter(moderation_items::state.eq_any(["open", "held"]))
            .group_by(moderation_items::subject_author_id)
            .select((
                moderation_items::subject_author_id,
                diesel::dsl::count(moderation_items::id),
            ))
            .load::<(String, i64)>(conn)?
            .into_iter()
            .collect();

        let mut out: Vec<UserRow> = rows
            .into_iter()
            .map(
                |(id, name, email, created, verified, totp, admin, moderator, deactivated)| {
                    let mut active = by_user.remove(&id).unwrap_or_default();
                    active.sort_by_key(|r| std::cmp::Reverse((r.kind == "ban") as i32));
                    let (status, ends) =
                        status_of(&active, deactivated.is_some(), verified.is_some());
                    UserRow {
                        last_seen_at: last_seen.get(&id).cloned().flatten(),
                        open_reports: open.get(&id).copied().unwrap_or(0),
                        id,
                        display_name: name,
                        email: pii.then_some(email),
                        created_at: created,
                        email_verified: verified.is_some(),
                        two_factor: totp.is_some(),
                        role: role_of(admin, moderator),
                        status: status.into(),
                        status_ends: ends,
                    }
                },
            )
            .filter(|u| query.status.as_deref().is_none_or(|s| u.status == s))
            .collect();

        // Newest first by whichever key; the id breaks ties so a row never
        // appears on two pages or on none.
        let key = |u: &UserRow| -> (String, String) {
            match sort.as_str() {
                "last_seen" => (u.last_seen_at.clone().unwrap_or_default(), u.id.clone()),
                "reports" => (format!("{:012}", u.open_reports), u.id.clone()),
                _ => (u.created_at.clone(), u.id.clone()),
            }
        };
        out.sort_by_key(|u| std::cmp::Reverse(key(u)));
        let start = match query.cursor.as_deref().and_then(|c| c.split_once('|')) {
            Some((value, id)) => {
                let after = (value.to_string(), id.to_string());
                out.iter().position(|u| key(u) < after).unwrap_or(out.len())
            }
            None => 0,
        };
        let more = out.len() > start + PAGE;
        let page: Vec<UserRow> = out.into_iter().skip(start).take(PAGE).collect();
        let next_cursor = more.then(|| {
            let (v, id) = key(page
                .last()
                .expect("a page that has a next page is not empty"));
            format!("{v}|{id}")
        });
        Ok(UserList {
            users: page,
            next_cursor,
        })
    })
    .await?;
    Ok(Json(page))
}

// --- one account -----------------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub display_name: String,
    pub bio: Option<String>,
    pub about_author: Option<String>,
    /// Administrators only, as are the next four.
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub legal_name: Option<String>,
    pub mailing_address: Option<String>,
    pub phone: Option<String>,
    pub created_at: String,
    pub last_seen_at: Option<String>,
    pub two_factor: bool,
    pub terms_version: Option<i32>,
    pub role: Option<StaffRole>,
    pub status: String,
    pub status_ends: Option<String>,
    pub deactivated_at: Option<String>,
    pub closing_at: Option<String>,
    pub purge_at: Option<String>,
    /// Made by staff and not yet set up by the person (administrators only).
    pub setup_pending: Option<bool>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub works: i64,
    pub publications: i64,
    pub comments: i64,
    pub critiques: i64,
    pub submissions: i64,
    pub invites_sent: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: String,
    pub user_agent: Option<String>,
    pub ip: Option<String>,
    pub created_at: String,
    pub last_seen_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReportAbout {
    pub item_id: String,
    pub subject_type: String,
    pub state: String,
    pub report_count: i32,
    pub top_reason: Option<String>,
    pub last_flagged_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReportBy {
    pub kind: String,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestrictionView {
    #[serde(flatten)]
    pub restriction: Restriction,
    pub in_force: bool,
    pub appeal: Option<Appeal>,
    pub created_by_name: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteView {
    pub id: String,
    pub body: String,
    pub author: AuthorCard,
    pub created_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuditRow {
    pub id: String,
    pub action: String,
    pub detail: String,
    pub reason: Option<String>,
    pub actor: Option<AuthorCard>,
    pub created_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UserDetail {
    pub summary: Summary,
    pub activity: Activity,
    /// Administrators only.
    pub sessions: Option<Vec<SessionRow>>,
    pub reports_about: Vec<ReportAbout>,
    pub reports_by: Vec<ReportBy>,
    pub restrictions: Vec<RestrictionView>,
    pub notes: Vec<NoteView>,
    pub audit: Vec<AuditRow>,
    /// What the viewer may do here, so the page shows only those buttons.
    pub actions: Vec<String>,
    /// The suggested next step on the escalation ladder.
    pub ladder: Vec<String>,
}

/// Actions on an account that a viewer with this role may take on it.
fn actions_for(staff: &StaffUser, target_flags: (bool, bool), target_id: &str) -> Vec<String> {
    let mut out = vec!["note".to_string()];
    let (admin, moderator) = target_flags;
    let allowed_target = staff.id() != target_id && !admin && (!moderator || staff.is_admin());
    if !allowed_target {
        return out;
    }
    if staff.can(Permission::RestrictDiscussion) {
        out.push("restrict".into());
        out.push("lift".into());
    }
    if staff.can(Permission::RestrictDiscussion) {
        out.push("edit_profile_text".into());
    }
    if staff.can(Permission::ManageAccounts) {
        for a in [
            "edit",
            "password_reset",
            "setup_email",
            "revoke_sessions",
            "deactivate",
            "reactivate",
            "delete",
            "delete_content",
        ] {
            out.push(a.into());
        }
    }
    if staff.can(Permission::ManageModerators) && !admin {
        out.push("moderator".into());
    }
    out
}

#[utoipa::path(
    get,
    path = "/api/admin/users/{user_id}",
    tag = "admin",
    params(("user_id" = String, Path)),
    responses(
        (status = 200, description = "The account, whole", body = UserDetail),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_user(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
) -> ApiResult<Json<UserDetail>> {
    staff.require(Permission::ViewQueue)?;
    let detail = blocking(&state.pool, move |conn| user_detail(conn, &staff, &user_id)).await?;
    Ok(Json(detail))
}

/// The columns the account page reads, in the order it reads them.
type DetailRow = (
    String,         // id
    String,         // display_name
    Option<String>, // bio
    Option<String>, // about_author
    String,         // email
    Option<String>, // email_verified_at
    Option<String>, // legal_name
    Option<String>, // mailing_address
    Option<String>, // phone
    String,         // created_at
    Option<String>, // totp_enabled_at
    Option<i32>,    // terms_version
    bool,           // is_admin
    bool,           // is_moderator
    Option<String>, // deactivated_at
    Option<String>, // closing_at
    Option<String>, // purge_at
    Option<String>, // terms_accepted_at
);

fn user_detail(conn: &mut Conn, staff: &StaffUser, user_id: &str) -> ApiResult<UserDetail> {
    let pii = staff.can(Permission::ViewAccountPii);
    let u: DetailRow = users::table
        .find(user_id)
        .select((
            users::id,
            users::display_name,
            users::bio,
            users::about_author,
            users::email,
            users::email_verified_at,
            users::legal_name,
            users::mailing_address,
            users::phone,
            users::created_at,
            users::totp_enabled_at,
            users::terms_version,
            users::is_admin,
            users::is_moderator,
            users::deactivated_at,
            users::closing_at,
            users::purge_at,
            users::terms_accepted_at,
        ))
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("no such account".into()))?;
    let (
        id,
        display_name,
        bio,
        about_author,
        email,
        verified,
        legal_name,
        mailing_address,
        phone,
        created_at,
        totp,
        terms_version,
        admin,
        moderator,
        deactivated_at,
        closing_at,
        purge_at,
        terms_accepted_at,
    ) = u;
    let active = rules::active(conn, &id)?;
    let (status, status_ends) = status_of(&active, deactivated_at.is_some(), verified.is_some());
    let last_seen_at: Option<String> = sessions::table
        .filter(sessions::user_id.eq(&id))
        .select(diesel::dsl::max(sessions::last_seen_at))
        .first(conn)?;
    let password_hash: String = users::table
        .find(&id)
        .select(users::password_hash)
        .first(conn)?;

    let summary = Summary {
        id: id.clone(),
        display_name,
        bio,
        about_author,
        email: pii.then(|| email.clone()),
        email_verified: pii.then_some(verified.is_some()),
        legal_name: if pii { legal_name } else { None },
        mailing_address: if pii { mailing_address } else { None },
        phone: if pii { phone } else { None },
        created_at,
        last_seen_at,
        two_factor: totp.is_some(),
        terms_version,
        role: role_of(admin, moderator),
        status: status.into(),
        status_ends,
        deactivated_at,
        closing_at,
        purge_at,
        setup_pending: pii.then_some(password_hash == "!" && terms_accepted_at.is_none()),
    };

    let count = |n: QueryResult<i64>| n.unwrap_or(0);
    let activity = Activity {
        works: count(
            works::table
                .filter(works::owner_id.eq(&id))
                .count()
                .get_result(conn),
        ),
        publications: count(
            publications::table
                .inner_join(works::table)
                .filter(works::owner_id.eq(&id))
                .count()
                .get_result(conn),
        ),
        comments: count(
            comments::table
                .filter(comments::author_id.eq(&id))
                .filter(comments::state.ne("deleted"))
                .count()
                .get_result(conn),
        ),
        critiques: count(
            prompt_critiques::table
                .filter(prompt_critiques::author_id.eq(&id))
                .filter(prompt_critiques::deleted_at.is_null())
                .count()
                .get_result(conn),
        ),
        submissions: count(
            prompt_entries::table
                .filter(prompt_entries::author_id.eq(&id))
                .filter(prompt_entries::state.eq("submitted"))
                .count()
                .get_result(conn),
        ),
        invites_sent: count(
            site_invites::table
                .filter(site_invites::inviter_id.eq(&id))
                .count()
                .get_result(conn),
        ),
    };

    let sessions_out = if pii {
        let rows: Vec<Session> = sessions::table
            .filter(sessions::user_id.eq(&id))
            .filter(sessions::revoked_at.is_null())
            .order(sessions::last_seen_at.desc())
            .select(Session::as_select())
            .load(conn)?;
        Some(
            rows.into_iter()
                .map(|s| SessionRow {
                    id: s.id,
                    user_agent: s.user_agent,
                    ip: s.ip,
                    created_at: s.created_at,
                    last_seen_at: s.last_seen_at,
                })
                .collect(),
        )
    } else {
        None
    };

    let reports_about: Vec<ReportAbout> = moderation_items::table
        .filter(moderation_items::subject_author_id.eq(&id))
        .order(moderation_items::last_flagged_at.desc())
        .limit(50)
        .select(ModerationItem::as_select())
        .load::<ModerationItem>(conn)?
        .into_iter()
        .map(|i| ReportAbout {
            item_id: i.id,
            subject_type: i.subject_type,
            state: i.state,
            report_count: i.report_count,
            top_reason: i.top_reason,
            last_flagged_at: i.last_flagged_at,
        })
        .collect();
    let mut reports_by: Vec<ReportBy> = Vec::new();
    for (kind, reason, at) in comment_reports::table
        .filter(comment_reports::reporter_id.eq(&id))
        .select((
            comment_reports::id,
            comment_reports::reason,
            comment_reports::created_at,
        ))
        .load::<(String, String, String)>(conn)?
        .into_iter()
        .map(|(_, r, a)| ("comment", r, a))
        .chain(
            prompt_critique_reports::table
                .filter(prompt_critique_reports::reporter_id.eq(&id))
                .select((
                    prompt_critique_reports::id,
                    prompt_critique_reports::reason,
                    prompt_critique_reports::created_at,
                ))
                .load::<(String, String, String)>(conn)?
                .into_iter()
                .map(|(_, r, a)| ("critique", r, a)),
        )
        .chain(
            reports::table
                .filter(reports::reporter_id.eq(&id))
                .select((reports::subject_type, reports::reason, reports::created_at))
                .load::<(String, String, String)>(conn)?
                .into_iter()
                .map(|(t, r, a)| ("other", format!("{r} ({t})"), a)),
        )
        .map(|(k, r, a)| (k.to_string(), r, a))
    {
        reports_by.push(ReportBy {
            kind,
            reason,
            created_at: at,
        });
    }
    reports_by.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    reports_by.truncate(50);

    let all: Vec<Restriction> = restrictions::table
        .filter(restrictions::user_id.eq(&id))
        .order(restrictions::created_at.desc())
        .select(Restriction::as_select())
        .load(conn)?;
    let ids: Vec<String> = all.iter().map(|r| r.id.clone()).collect();
    let appeal_rows: Vec<Appeal> = appeals::table
        .filter(appeals::restriction_id.eq_any(&ids))
        .select(Appeal::as_select())
        .load(conn)?;
    let setters: Vec<String> = all.iter().map(|r| r.created_by.clone()).collect();
    let cards = support::author_cards(conn, &setters)?;
    let now = clock::now_iso();
    let restrictions_out: Vec<RestrictionView> = all
        .into_iter()
        .map(|r| RestrictionView {
            in_force: rules::in_force(&r, &now),
            appeal: appeal_rows
                .iter()
                .find(|a| a.restriction_id == r.id)
                .cloned(),
            created_by_name: cards.get(&r.created_by).map(|c| c.display_name.clone()),
            restriction: r,
        })
        .collect();

    let note_rows: Vec<StaffNote> = staff_notes::table
        .filter(staff_notes::user_id.eq(&id))
        .order(staff_notes::created_at.desc())
        .select(StaffNote::as_select())
        .load(conn)?;
    let note_authors: Vec<String> = note_rows.iter().map(|n| n.author_id.clone()).collect();
    let note_cards = support::author_cards(conn, &note_authors)?;
    let notes = note_rows
        .into_iter()
        .map(|n| NoteView {
            author: note_cards.get(&n.author_id).cloned().unwrap_or(AuthorCard {
                id: n.author_id.clone(),
                display_name: "Former staff member".into(),
            }),
            id: n.id,
            body: n.body,
            created_at: n.created_at,
        })
        .collect();

    // Moderators see what staff did to the account in the moderation log, and
    // not the account's own security history.
    let mut audit_q = audit_events::table
        .filter(audit_events::subject_id.eq(&id))
        .into_boxed();
    if pii {
        audit_q = audit_events::table
            .filter(
                audit_events::subject_id
                    .eq(&id)
                    .or(audit_events::actor_id.eq(&id)),
            )
            .into_boxed();
    }
    let audit_rows: Vec<StaffAuditRow> = audit_q
        .order(audit_events::created_at.desc())
        .limit(100)
        .select(StaffAuditRow::as_select())
        .load(conn)?;
    let actors: Vec<String> = audit_rows
        .iter()
        .filter_map(|a| a.actor_id.clone())
        .collect();
    let actor_cards = support::author_cards(conn, &actors)?;
    let audit = audit_rows
        .into_iter()
        .map(|a| AuditRow {
            actor: a.actor_id.as_ref().map(|actor| {
                actor_cards.get(actor).cloned().unwrap_or(AuthorCard {
                    id: actor.clone(),
                    display_name: if actor.starts_with("external:") {
                        actor.clone()
                    } else {
                        "Former staff member".into()
                    },
                })
            }),
            id: a.id,
            action: a.action,
            detail: a.detail,
            reason: a.reason,
            created_at: a.created_at,
        })
        .collect();

    Ok(UserDetail {
        summary,
        activity,
        sessions: sessions_out,
        reports_about,
        reports_by,
        restrictions: restrictions_out,
        notes,
        audit,
        actions: actions_for(staff, (admin, moderator), &id),
        ladder: crate::domain::moderation::ladder(conn)?,
    })
}

// --- notes and moderators --------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoteRequest {
    pub body: String,
}

#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/notes",
    tag = "admin",
    params(("user_id" = String, Path)),
    request_body = NoteRequest,
    responses(
        (status = 200, description = "Added", body = NoteView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn add_note(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
    Json(body): Json<NoteRequest>,
) -> ApiResult<Json<NoteView>> {
    staff.require(Permission::ViewQueue)?;
    let text = body.body.trim().to_string();
    if text.is_empty() || text.chars().count() > 5_000 {
        return Err(ApiError::BadRequest(
            "a note is between 1 and 5,000 characters".into(),
        ));
    }
    let view = blocking(&state.pool, move |conn| {
        staff_flags(conn, &user_id)?;
        let note = StaffNote {
            id: new_id(),
            user_id: user_id.clone(),
            author_id: staff.id().to_string(),
            body: text,
            created_at: clock::now_iso(),
        };
        diesel::insert_into(staff_notes::table)
            .values((
                staff_notes::id.eq(&note.id),
                staff_notes::user_id.eq(&note.user_id),
                staff_notes::author_id.eq(&note.author_id),
                staff_notes::body.eq(&note.body),
                staff_notes::created_at.eq(&note.created_at),
            ))
            .execute(conn)?;
        record_staff(
            conn,
            staff.id(),
            Some(&user_id),
            AuditAction::StaffNoteAdded,
            "a note was added",
            None,
        )?;
        let card = support::author_cards(conn, &[staff.id().to_string()])?
            .remove(staff.id())
            .unwrap_or(AuthorCard {
                id: staff.id().into(),
                display_name: "Staff".into(),
            });
        Ok(NoteView {
            id: note.id,
            body: note.body,
            author: card,
            created_at: note.created_at,
        })
    })
    .await?;
    Ok(Json(view))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModeratorRequest {
    pub moderator: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/moderator",
    tag = "admin",
    params(("user_id" = String, Path)),
    request_body = ModeratorRequest,
    responses(
        (status = 200, description = "Done", body = crate::routes::account::Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn set_moderator(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
    Json(body): Json<ModeratorRequest>,
) -> ApiResult<Json<super::account::Done>> {
    staff.require(Permission::ManageModerators)?;
    let reason = clean_reason(&body.reason)?;
    blocking(&state.pool, move |conn| {
        guard_target(conn, &staff, &user_id)?;
        let (_, was) = staff_flags(conn, &user_id)?;
        if was == body.moderator {
            return Err(ApiError::Conflict(if was {
                "they are already a moderator".into()
            } else {
                "they are not a moderator".into()
            }));
        }
        diesel::update(users::table.find(&user_id))
            .set(users::is_moderator.eq(body.moderator))
            .execute(conn)?;
        record_staff(
            conn,
            staff.id(),
            Some(&user_id),
            if body.moderator {
                AuditAction::ModeratorGranted
            } else {
                AuditAction::ModeratorRevoked
            },
            "",
            Some(&reason),
        )?;
        Ok(())
    })
    .await?;
    Ok(Json(super::account::Done { ok: true }))
}

// --- making an account for someone -------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub email: String,
    pub display_name: String,
    #[serde(default)]
    pub moderator: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    pub id: String,
    pub display_name: String,
    pub email: String,
}

/// The mail with the link a person sets their own password from.
fn setup_message(public_url: &str, to: &str, name: &str, token: &str) -> Message {
    Message {
        to: to.into(),
        subject: "Set up your Just Type Studio account".into(),
        body: format!(
            "Hello {name},\n\nSupport made an account for you on Just Type Studio. Set your password and agree to the terms here:\n\n{public_url}/setup?token={token}\n\nThe link lasts {SETUP_DAYS} days and works once. If you weren't expecting this, ignore it and nothing happens.\n"
        ),
        ..Default::default()
    }
}

#[utoipa::path(
    post,
    path = "/api/admin/users",
    tag = "admin",
    request_body = CreateUserRequest,
    responses(
        (status = 200, description = "Created; the person is emailed a link", body = Created),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn create_user(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<CreateUserRequest>,
) -> ApiResult<Json<Created>> {
    staff.require(Permission::ManageAccounts)?;
    if body.moderator {
        staff.require(Permission::ManageModerators)?;
    }
    let email = normalize_email(&body.email)?;
    let name = body.display_name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::BadRequest("a display name is required".into()));
    }
    let public_url = state.config.public_url.clone();
    let created = blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            if users::table
                .filter(users::email.eq(&email))
                .count()
                .get_result::<i64>(conn)?
                > 0
            {
                return Err(ApiError::Conflict(
                    "that address already has an account".into(),
                ));
            }
            if let Some(ban) = crate::domain::bans::banned_address(conn, &email)? {
                return Err(ApiError::Conflict(format!(
                    "that address is on the permanent ban list ({})",
                    ban.reason_text
                )));
            }
            if crate::domain::bans::blocked_domain(conn, &email)?.is_some() {
                return Err(ApiError::Conflict(
                    "that address is at a blocked domain".into(),
                ));
            }
            let now = clock::now_iso();
            let id = new_id();
            diesel::insert_into(users::table)
                .values(&NewUser {
                    id: id.clone(),
                    email: email.clone(),
                    display_name: name.clone(),
                    // Not a hash, so nobody can sign in until the person sets
                    // their own password from the emailed link.
                    password_hash: "!".into(),
                    bio: None,
                    created_at: now.clone(),
                    updated_at: now,
                    daily_word_goal: None,
                    legal_name: None,
                    mailing_address: None,
                    phone: None,
                    email_verified_at: None,
                    totp_secret: None,
                    totp_enabled_at: None,
                    totp_last_step: None,
                    terms_version: None,
                    terms_accepted_at: None,
                    weekly_word_goal: None,
                    summaries_allowed_at: None,
                    assistant_allowed_at: None,
                })
                .execute(conn)?;
            if body.moderator {
                diesel::update(users::table.find(&id))
                    .set(users::is_moderator.eq(true))
                    .execute(conn)?;
            }
            let user = support::find_user(conn, &id)?;
            let token = mint(conn, &user, "setup", chrono::Duration::days(SETUP_DAYS))?;
            crate::mail::enqueue(
                conn,
                &setup_message(&public_url, &email, &name, &token),
                clock::now() + chrono::Duration::days(3),
            )?;
            record_staff(
                conn,
                staff.id(),
                Some(&id),
                AuditAction::AccountCreatedByStaff,
                &format!(
                    "{name}{}",
                    if body.moderator {
                        ", as a moderator"
                    } else {
                        ""
                    }
                ),
                None,
            )?;
            if body.moderator {
                record_staff(
                    conn,
                    staff.id(),
                    Some(&id),
                    AuditAction::ModeratorGranted,
                    "",
                    Some("made with the account"),
                )?;
            }
            Ok(Created {
                id,
                display_name: name,
                email,
            })
        })
    })
    .await?;
    Ok(Json(created))
}

#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/setup-email",
    tag = "admin",
    params(("user_id" = String, Path)),
    responses(
        (status = 200, description = "Sent again", body = crate::routes::account::Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn resend_setup(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
) -> ApiResult<Json<super::account::Done>> {
    staff.require(Permission::ManageAccounts)?;
    let public_url = state.config.public_url.clone();
    blocking(&state.pool, move |conn| {
        guard_target(conn, &staff, &user_id)?;
        let user = support::find_user(conn, &user_id)?;
        if user.password_hash != "!" || user.terms_accepted_at.is_some() {
            return Err(ApiError::Conflict(
                "that account has already been set up".into(),
            ));
        }
        let token = mint(conn, &user, "setup", chrono::Duration::days(SETUP_DAYS))?;
        crate::mail::enqueue(
            conn,
            &setup_message(&public_url, &user.email, &user.display_name, &token),
            clock::now() + chrono::Duration::days(3),
        )?;
        record_staff(
            conn,
            staff.id(),
            Some(&user_id),
            AuditAction::AccountCreatedByStaff,
            "setup email sent again",
            None,
        )?;
        Ok(())
    })
    .await?;
    Ok(Json(super::account::Done { ok: true }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetupRequest {
    pub token: String,
    pub password: String,
    #[serde(default)]
    pub accept_terms: bool,
}

/// Following the setup link: the person chooses a password, agrees to the
/// terms, and is signed in. Choosing it also proves the address, because they
/// read the mail.
#[utoipa::path(
    post,
    path = "/api/auth/setup",
    tag = "auth",
    request_body = SetupRequest,
    responses(
        (status = 200, description = "Set up and signed in", body = super::auth::SessionResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn setup(
    State(state): State<AppState>,
    client: auth::ClientInfo,
    Json(body): Json<SetupRequest>,
) -> ApiResult<Json<super::auth::SessionResponse>> {
    super::auth::check_password(&body.password)?;
    if !body.accept_terms {
        return Err(ApiError::BadRequest(
            "an account needs agreement to the terms — they say your writing stays yours".into(),
        ));
    }
    let hash = auth::password::hash(&body.password)?;
    let inner = state.clone();
    let session = blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            let user = super::account::redeem(conn, &body.token, "setup")?;
            let now = clock::now_iso();
            diesel::update(users::table.find(&user.id))
                .set((
                    users::password_hash.eq(&hash),
                    users::updated_at.eq(&now),
                    users::email_verified_at.eq(Some(now.clone())),
                ))
                .execute(conn)?;
            super::privacy::record_terms(conn, &user.id)?;
            let user = support::find_user(conn, &user.id)?;
            super::auth::start_session(&inner, conn, user, &client)
        })
    })
    .await?;
    Ok(Json(session))
}

// --- editing -----------------------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditUserRequest {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub about_author: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub mark_verified: Option<bool>,
    #[serde(default)]
    pub reset_two_factor: Option<bool>,
    /// Required for every change.
    #[serde(default)]
    pub reason: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/api/admin/users/{user_id}",
    tag = "admin",
    params(("user_id" = String, Path)),
    request_body = EditUserRequest,
    responses(
        (status = 200, description = "Changed", body = UserDetail),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn edit_user(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
    Json(body): Json<EditUserRequest>,
) -> ApiResult<Json<UserDetail>> {
    let text_change =
        body.display_name.is_some() || body.bio.is_some() || body.about_author.is_some();
    let account_change = body.email.is_some()
        || body.mark_verified == Some(true)
        || body.reset_two_factor == Some(true);
    if !text_change && !account_change {
        return Err(ApiError::BadRequest("there is nothing to change".into()));
    }
    // A moderator may fix text that breaks the guidelines, and nothing else.
    if text_change {
        staff.require(Permission::RestrictDiscussion)?;
    }
    if account_change {
        staff.require(Permission::ManageAccounts)?;
    }
    let reason = clean_reason(&body.reason)?;
    let public_url = state.config.public_url.clone();
    let detail = blocking(&state.pool, move |conn| {
        guard_target(conn, &staff, &user_id)?;
        conn.transaction::<_, ApiError, _>(|conn| {
            let user = support::find_user(conn, &user_id)?;
            let now = clock::now_iso();
            let note = |conn: &mut Conn, field: &str, detail: String| {
                record_staff(
                    conn,
                    staff.id(),
                    Some(&user_id),
                    AuditAction::AccountEdited,
                    &format!("{field}: {detail}"),
                    Some(&reason),
                )
            };
            if let Some(name) = body.display_name.as_deref() {
                let name = name.trim();
                if name.is_empty() {
                    return Err(ApiError::BadRequest("a display name is required".into()));
                }
                diesel::update(users::table.find(&user_id))
                    .set((users::display_name.eq(name), users::updated_at.eq(&now)))
                    .execute(conn)?;
                note(conn, "display name", format!("“{}” → “{name}”", user.display_name))?;
            }
            if let Some(bio) = body.bio.as_deref() {
                diesel::update(users::table.find(&user_id))
                    .set((users::bio.eq(Some(bio.trim().to_string())), users::updated_at.eq(&now)))
                    .execute(conn)?;
                note(conn, "bio", format!("“{}” → “{}”", user.bio.clone().unwrap_or_default(), bio.trim()))?;
            }
            if let Some(about) = body.about_author.as_deref() {
                let old: Option<String> = users::table
                    .find(&user_id)
                    .select(users::about_author)
                    .first(conn)?;
                diesel::update(users::table.find(&user_id))
                    .set((
                        users::about_author.eq(Some(about.trim().to_string())),
                        users::updated_at.eq(&now),
                    ))
                    .execute(conn)?;
                note(conn, "about the author", format!("“{}” → “{}”", old.unwrap_or_default(), about.trim()))?;
            }
            if let Some(new_email) = body.email.as_deref() {
                let new_email = normalize_email(new_email)?;
                if new_email != user.email {
                    if users::table
                        .filter(users::email.eq(&new_email))
                        .count()
                        .get_result::<i64>(conn)?
                        > 0
                    {
                        return Err(ApiError::Conflict("that address already has an account".into()));
                    }
                    if let Some(ban) = crate::domain::bans::banned_address(conn, &new_email)? {
                        return Err(ApiError::Conflict(format!(
                            "that address is on the permanent ban list ({})",
                            ban.reason_text
                        )));
                    }
                    if crate::domain::bans::blocked_domain(conn, &new_email)?.is_some() {
                        return Err(ApiError::Conflict("that address is at a blocked domain".into()));
                    }
                    diesel::update(users::table.find(&user_id))
                        .set((
                            users::email.eq(&new_email),
                            users::email_verified_at.eq(None::<String>),
                            users::updated_at.eq(&now),
                        ))
                        .execute(conn)?;
                    let updated = support::find_user(conn, &user_id)?;
                    let token = mint(conn, &updated, "verify", chrono::Duration::hours(72))?;
                    let give_up = clock::now() + chrono::Duration::days(3);
                    crate::mail::enqueue(
                        conn,
                        &Message {
                            to: user.email.clone(),
                            subject: "The address on your Just Type Studio account was changed".into(),
                            body: format!(
                                "Hello {},\n\nSupport changed the address on your account to {new_email}. If you didn't ask for this, reply to this message.\n",
                                user.display_name
                            ),
                            ..Default::default()
                        },
                        give_up,
                    )?;
                    crate::mail::enqueue(
                        conn,
                        &Message {
                            to: new_email.clone(),
                            subject: "Confirm your address for Just Type Studio".into(),
                            body: format!(
                                "Hello {},\n\nConfirm this is your address:\n\n{public_url}/verify?token={token}\n\nThe link lasts 72 hours.\n",
                                user.display_name
                            ),
                            ..Default::default()
                        },
                        give_up,
                    )?;
                    note(conn, "email", "changed; both addresses were told".into())?;
                }
            }
            if body.mark_verified == Some(true) && user.email_verified_at.is_none() {
                diesel::update(users::table.find(&user_id))
                    .set(users::email_verified_at.eq(Some(now.clone())))
                    .execute(conn)?;
                note(conn, "address", "marked as confirmed".into())?;
            }
            if body.reset_two_factor == Some(true) {
                if user.totp_enabled_at.is_none() {
                    return Err(ApiError::Conflict("two-factor sign-in is not on".into()));
                }
                diesel::update(users::table.find(&user_id))
                    .set((
                        users::totp_secret.eq(None::<String>),
                        users::totp_enabled_at.eq(None::<String>),
                        users::totp_last_step.eq(None::<i64>),
                    ))
                    .execute(conn)?;
                diesel::delete(recovery_codes::table.filter(recovery_codes::user_id.eq(&user_id)))
                    .execute(conn)?;
                revoke_sessions(conn, &user_id, None)?;
                crate::mail::enqueue(
                    conn,
                    &Message {
                        to: user.email.clone(),
                        subject: "Two-factor sign-in was turned off on your account".into(),
                        body: format!(
                            "Hello {},\n\nSupport turned off two-factor sign-in on your account and signed out every device. You can turn it on again in Settings. If you didn't ask for this, reply to this message.\n",
                            user.display_name
                        ),
                        ..Default::default()
                    },
                    clock::now() + chrono::Duration::days(3),
                )?;
                record_staff(
                    conn,
                    staff.id(),
                    Some(&user_id),
                    AuditAction::TwoFactorDisabled,
                    &format!("by staff: {reason}"),
                    Some(&reason),
                )?;
            }
            Ok(())
        })?;
        user_detail(conn, &staff, &user_id)
    })
    .await?;
    Ok(Json(detail))
}

// --- sign-out and reset -------------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResetRequest {
    /// Also sign out every device now.
    #[serde(default)]
    pub sign_out: bool,
}

#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/password-reset",
    tag = "admin",
    params(("user_id" = String, Path)),
    request_body = ResetRequest,
    responses(
        (status = 200, description = "The reset email was sent", body = crate::routes::account::Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn send_reset(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
    Json(body): Json<ResetRequest>,
) -> ApiResult<Json<super::account::Done>> {
    staff.require(Permission::ManageAccounts)?;
    let public_url = state.config.public_url.clone();
    blocking(&state.pool, move |conn| {
        guard_target(conn, &staff, &user_id)?;
        let user = support::find_user(conn, &user_id)?;
        // Three an hour per account, however many staff are asking.
        let since = (clock::now() - chrono::Duration::hours(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let recent: i64 = audit_events::table
            .filter(audit_events::subject_id.eq(&user_id))
            .filter(audit_events::action.eq(AuditAction::PasswordResetSent.as_str()))
            .filter(audit_events::created_at.gt(&since))
            .count()
            .get_result(conn)?;
        if recent >= RESETS_PER_HOUR {
            return Err(ApiError::TooManyRequests {
                message: format!("{RESETS_PER_HOUR} resets an hour is the most for one account"),
                retry_after: 3600,
            });
        }
        conn.transaction::<_, ApiError, _>(|conn| {
            let token = mint(conn, &user, "reset", chrono::Duration::minutes(RESET_MINUTES))?;
            crate::mail::enqueue(
                conn,
                &Message {
                    to: user.email.clone(),
                    subject: "Resetting your Just Type Studio password".into(),
                    body: format!(
                        "Hello {},\n\nSupport sent this to help you get back into your account:\n\n{public_url}/reset?token={token}\n\nThe link lasts {RESET_MINUTES} minutes and works once. Using it signs out every device. If you didn't ask support for help, ignore this; your password has not changed.\n",
                        user.display_name
                    ),
                    ..Default::default()
                },
                clock::now() + chrono::Duration::hours(2),
            )?;
            record_staff(
                conn,
                staff.id(),
                Some(&user_id),
                AuditAction::PasswordResetSent,
                if body.sign_out { "and every device signed out" } else { "" },
                None,
            )?;
            if body.sign_out {
                let ended = revoke_sessions(conn, &user_id, None)?;
                record_staff(
                    conn,
                    staff.id(),
                    Some(&user_id),
                    AuditAction::SessionsRevokedByStaff,
                    &format!("{ended} session(s), with the reset"),
                    None,
                )?;
            }
            Ok(())
        })
    })
    .await?;
    Ok(Json(super::account::Done { ok: true }))
}

#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/sessions/revoke-all",
    tag = "admin",
    params(("user_id" = String, Path)),
    responses(
        (status = 200, description = "Every device signed out", body = crate::routes::account::Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn revoke_all(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
) -> ApiResult<Json<super::account::Done>> {
    staff.require(Permission::ManageAccounts)?;
    blocking(&state.pool, move |conn| {
        guard_target(conn, &staff, &user_id)?;
        let ended = revoke_sessions(conn, &user_id, None)?;
        record_staff(
            conn,
            staff.id(),
            Some(&user_id),
            AuditAction::SessionsRevokedByStaff,
            &format!("{ended} session(s)"),
            None,
        )
    })
    .await?;
    Ok(Json(super::account::Done { ok: true }))
}

#[utoipa::path(
    delete,
    path = "/api/admin/users/{user_id}/sessions/{session_id}",
    tag = "admin",
    params(("user_id" = String, Path), ("session_id" = String, Path)),
    responses(
        (status = 200, description = "Signed out", body = crate::routes::account::Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn revoke_one(
    State(state): State<AppState>,
    staff: StaffUser,
    Path((user_id, session_id)): Path<(String, String)>,
) -> ApiResult<Json<super::account::Done>> {
    staff.require(Permission::ManageAccounts)?;
    blocking(&state.pool, move |conn| {
        guard_target(conn, &staff, &user_id)?;
        let now = clock::now_iso();
        let ended = diesel::update(
            sessions::table
                .find(&session_id)
                .filter(sessions::user_id.eq(&user_id))
                .filter(sessions::revoked_at.is_null()),
        )
        .set(sessions::revoked_at.eq(&now))
        .execute(conn)?;
        if ended == 0 {
            return Err(ApiError::NotFound("no such session".into()));
        }
        diesel::update(
            refresh_tokens::table
                .filter(refresh_tokens::session_id.eq(&session_id))
                .filter(refresh_tokens::revoked_at.is_null()),
        )
        .set(refresh_tokens::revoked_at.eq(&now))
        .execute(conn)?;
        record_staff(
            conn,
            staff.id(),
            Some(&user_id),
            AuditAction::SessionsRevokedByStaff,
            "1 session",
            None,
        )
    })
    .await?;
    Ok(Json(super::account::Done { ok: true }))
}

// --- deactivating and deleting -----------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReasonRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/deactivate",
    tag = "admin",
    params(("user_id" = String, Path)),
    request_body = ReasonRequest,
    responses(
        (status = 200, description = "Deactivated", body = crate::routes::account::Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn deactivate(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
    Json(body): Json<ReasonRequest>,
) -> ApiResult<Json<super::account::Done>> {
    staff.require(Permission::ManageAccounts)?;
    let reason = clean_reason(&body.reason)?;
    blocking(&state.pool, move |conn| {
        guard_target(conn, &staff, &user_id)?;
        conn.transaction::<_, ApiError, _>(|conn| {
            let done = diesel::update(
                users::table
                    .find(&user_id)
                    .filter(users::deactivated_at.is_null()),
            )
            .set(users::deactivated_at.eq(Some(clock::now_iso())))
            .execute(conn)?;
            if done == 0 {
                return Err(ApiError::Conflict(
                    "that account is already deactivated".into(),
                ));
            }
            revoke_sessions(conn, &user_id, None)?;
            record_staff(
                conn,
                staff.id(),
                Some(&user_id),
                AuditAction::AccountDeactivated,
                "",
                Some(&reason),
            )
        })
    })
    .await?;
    Ok(Json(super::account::Done { ok: true }))
}

#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/reactivate",
    tag = "admin",
    params(("user_id" = String, Path)),
    request_body = ReasonRequest,
    responses(
        (status = 200, description = "Reactivated", body = crate::routes::account::Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn reactivate(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
    Json(body): Json<ReasonRequest>,
) -> ApiResult<Json<super::account::Done>> {
    staff.require(Permission::ManageAccounts)?;
    let reason = clean_reason(&body.reason)?;
    blocking(&state.pool, move |conn| {
        guard_target(conn, &staff, &user_id)?;
        let done = diesel::update(
            users::table
                .find(&user_id)
                .filter(users::deactivated_at.is_not_null()),
        )
        .set(users::deactivated_at.eq(None::<String>))
        .execute(conn)?;
        if done == 0 {
            return Err(ApiError::Conflict("that account is not deactivated".into()));
        }
        record_staff(
            conn,
            staff.id(),
            Some(&user_id),
            AuditAction::AccountReactivated,
            "",
            Some(&reason),
        )
    })
    .await?;
    Ok(Json(super::account::Done { ok: true }))
}

#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/delete-content",
    tag = "admin",
    params(("user_id" = String, Path)),
    request_body = ReasonRequest,
    responses(
        (status = 200, description = "Removed, with how many", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_content(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
    Json(body): Json<ReasonRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    staff.require(Permission::ManageAccounts)?;
    let reason = clean_reason(&body.reason)?;
    let count = blocking(&state.pool, move |conn| {
        guard_target(conn, &staff, &user_id)?;
        conn.transaction::<_, ApiError, _>(|conn| {
            let n = crate::domain::moderation::remove_all_discussion(conn, &user_id)?;
            record_staff(
                conn,
                staff.id(),
                Some(&user_id),
                AuditAction::AccountContentRemoved,
                &format!("{n} public item(s) marked removed"),
                Some(&reason),
            )?;
            Ok(n)
        })
    })
    .await?;
    Ok(Json(json!({ "removed": count })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeleteUserRequest {
    /// The account's display name, typed to confirm.
    pub confirm_name: String,
    #[serde(default)]
    pub reason: Option<String>,
    /// Go ahead although other people are working on books this account owns.
    #[serde(default)]
    pub delete_shared_works: bool,
    /// Don't email the person a copy of their data first. Needs the reason
    /// above to say why (a spam account, say).
    #[serde(default)]
    pub skip_export: bool,
}

#[utoipa::path(
    delete,
    path = "/api/admin/users/{user_id}",
    tag = "admin",
    params(("user_id" = String, Path)),
    request_body = DeleteUserRequest,
    responses(
        (status = 200, description = "Gone", body = super::privacy::DeletedAccount),
        (status = 409, description = "Books with co-authors would go too", body = ErrorEnvelope),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_user(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
    Json(body): Json<DeleteUserRequest>,
) -> ApiResult<Json<super::privacy::DeletedAccount>> {
    staff.require(Permission::ManageAccounts)?;
    let reason = clean_reason(&body.reason)?;
    let id = user_id.clone();
    let (found, shared) = blocking(&state.pool, {
        let staff = staff.clone();
        move |conn| {
            guard_target(conn, &staff, &id)?;
            let found = support::find_user(conn, &id)?;
            let shared = super::privacy::shared_titles(conn, &id)?;
            Ok((found, shared))
        }
    })
    .await?;
    if body.confirm_name.trim() != found.display_name.trim() {
        return Err(ApiError::BadRequest(
            "type the account's display name to confirm".into(),
        ));
    }
    if !shared.is_empty() && !body.delete_shared_works {
        return Err(ApiError::ConflictWith {
            message: format!(
                "other people are working on {} — deleting the account deletes {} for them too",
                shared.join(", "),
                if shared.len() == 1 { "it" } else { "them" }
            ),
            details: json!({ "sharedWorks": shared }),
        });
    }
    // The copy goes out first: once the account is gone there is nothing to
    // build it from.
    if !body.skip_export {
        super::privacy::email_export(&state, &found.id, &found.email, &found.display_name, 7)
            .await?;
    }
    let found_id = found.id.clone();
    let owned = blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            let removed = super::privacy::remove_account(conn, &found, false)?;
            record_staff(
                conn,
                staff.id(),
                Some(&found.id),
                AuditAction::AccountDeletedByStaff,
                &format!(
                    "{} work(s) and their files deleted{}",
                    removed.deleted.len(),
                    if body.skip_export {
                        "; no export was sent"
                    } else {
                        "; an export was emailed"
                    }
                ),
                Some(&reason),
            )?;
            Ok(removed.deleted.into_iter().map(|w| w.0).collect::<Vec<_>>())
        })
    })
    .await?;
    let count = owned.len();
    for work_id in owned {
        if let Err(e) = state.store.delete_work(&work_id).await {
            tracing::error!(%work_id, error = %e, "account deleted but its work's files remain");
        }
    }
    tracing::info!(user = %found_id, "an account was deleted by staff");
    Ok(Json(super::privacy::DeletedAccount {
        works_deleted: count,
    }))
}
