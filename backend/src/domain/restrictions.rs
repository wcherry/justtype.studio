//! Restrictions: what an account may not do, decided in one place.
//!
//! A restriction is a row in `restrictions`. It is in force when
//! `starts_at <= now AND (ends_at IS NULL OR ends_at > now) AND lifted_at IS
//! NULL`, compared against [`clock::now`], so a temporary one simply stops
//! applying at `ends_at` without any job running. The `expire_restrictions`
//! job only sends the *it has ended* notice.
//!
//! Enforcement is a single chokepoint, [`enforce`], a middleware over every
//! route. It looks the route up in [`GATES`], which names a [`Capability`] for
//! every mutating route in the API (or says the route is never restricted),
//! and asks [`can`]. A test walks the OpenAPI document and fails when a route
//! is missing from the table, so a new write route cannot forget the check.
use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Duration, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};

use crate::clock;
use crate::db::models::Restriction;
use crate::db::schema::{restrictions, users};
use crate::db::{blocking, Conn};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// The days a closing restriction waits before the account is deleted.
pub const CLOSING_DAYS: i64 = 30;

/// The only lengths a restriction may have, in days. `None` is permanent.
pub const DURATIONS: &[Option<i64>] = &[Some(3), Some(7), Some(30), None];

pub const KINDS: &[&str] = &["warning", "ban", "suspend", "read_only"];
pub const SCOPES: &[&str] = &["site", "discussion"];

/// What a restricted account is refused. Each mutating route names one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Edit your own work, settings and lists.
    Write,
    /// Make something public: publish a version, open a publication.
    Publish,
    /// Comments, votes, reactions, answers, Daily Prompts submissions and
    /// critiques.
    Discuss,
    /// Report something.
    Report,
    /// Invite someone to the site.
    Invite,
    /// Follow a writer.
    Follow,
}

impl Capability {
    pub fn as_str(self) -> &'static str {
        match self {
            Capability::Write => "write",
            Capability::Publish => "publish",
            Capability::Discuss => "discuss",
            Capability::Report => "report",
            Capability::Invite => "invite",
            Capability::Follow => "follow",
        }
    }
}

/// Whether `r` takes `capability` away while it is in force.
fn denies(r: &Restriction, capability: Capability) -> bool {
    use Capability::*;
    match (r.kind.as_str(), r.scope.as_str()) {
        ("warning", _) => false,
        ("ban", "site") | ("read_only", "site") => true,
        ("suspend", "site") => !matches!(capability, Write),
        // Within discussions suspend, read-only and ban are the same: the
        // names are kept so the reason shown to the writer fits what happened.
        (_, "discussion") => matches!(capability, Discuss | Report | Invite | Follow),
        _ => false,
    }
}

/// How hard a restriction is, to pick the strictest when there are several.
fn weight(r: &Restriction) -> i32 {
    let kind = match r.kind.as_str() {
        "ban" => 30,
        "read_only" => 20,
        "suspend" => 10,
        _ => 0,
    };
    let scope = if r.scope == "site" { 100 } else { 0 };
    kind + scope
}

pub fn in_force(r: &Restriction, now: &str) -> bool {
    r.starts_at.as_str() <= now
        && r.lifted_at.is_none()
        && r.ends_at.as_deref().is_none_or(|end| end > now)
}

/// Every restriction in force on the account now (warnings excluded: they
/// have no effect), strictest first.
pub fn active(conn: &mut Conn, user_id: &str) -> ApiResult<Vec<Restriction>> {
    let now = clock::now_iso();
    let mut rows: Vec<Restriction> = restrictions::table
        .filter(restrictions::user_id.eq(user_id))
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
    rows.sort_by_key(|r| std::cmp::Reverse(weight(r)));
    Ok(rows)
}

/// What a client is told about a restriction.
pub fn describe(r: &Restriction) -> Value {
    json!({
        "restrictionId": r.id,
        "kind": r.kind,
        "scope": r.scope,
        "until": r.ends_at,
        "reasonCode": r.reason_code,
        "reason": r.reason_text,
    })
}

/// The site ban in force on the account, if any.
pub fn site_ban(rows: &[Restriction]) -> Option<&Restriction> {
    rows.iter().find(|r| r.kind == "ban" && r.scope == "site")
}

/// The refusal for a deactivated account.
pub fn deactivated_error() -> ApiError {
    ApiError::Coded {
        code: "account_deactivated",
        message: "this account has been deactivated — contact support".into(),
        details: json!({}),
    }
}

/// The refusal for a banned account, at sign-in and on every request.
pub fn banned(r: &Restriction) -> ApiError {
    ApiError::Coded {
        code: "account_banned",
        message: match &r.ends_at {
            Some(end) => format!("this account is banned until {end}: {}", r.reason_text),
            None => format!("this account is banned: {}", r.reason_text),
        },
        details: describe(r),
    }
}

/// `Ok` if none of the restrictions in `rows` takes `capability` away.
pub fn can(rows: &[Restriction], capability: Capability) -> ApiResult<()> {
    match rows.iter().find(|r| denies(r, capability)) {
        None => Ok(()),
        Some(r) => {
            let mut details = describe(r);
            details["capability"] = json!(capability.as_str());
            Err(ApiError::Coded {
                code: "account_restricted",
                message: match &r.ends_at {
                    Some(end) => format!("you can't do that until {end}: {}", r.reason_text),
                    None => format!("you can't do that on this account: {}", r.reason_text),
                },
                details,
            })
        }
    }
}

/// The site-wide read-only restriction that is counting down to deletion.
pub fn closing(rows: &[Restriction]) -> Option<&Restriction> {
    rows.iter().find(|r| is_closing(r))
}

/// A closing restriction: read-only on the whole site, or a permanent site
/// ban. Both end in the account being deleted.
pub fn is_closing(r: &Restriction) -> bool {
    r.scope == "site" && (r.kind == "read_only" || (r.kind == "ban" && r.ends_at.is_none()))
}

/// The end of a new restriction's life, given its length.
pub fn ends_at(start: DateTime<Utc>, days: Option<i64>) -> Option<String> {
    days.map(|d| (start + Duration::days(d)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// Users whose public things nobody else sees: deactivated accounts, and
/// accounts under a permanent site ban (hidden at once, deleted after 30
/// days). Small enough to read whole.
pub fn hidden_users(conn: &mut Conn) -> ApiResult<Vec<String>> {
    let now = clock::now_iso();
    let mut ids: Vec<String> = users::table
        .filter(users::deactivated_at.is_not_null())
        .select(users::id)
        .load(conn)?;
    let banned: Vec<String> = restrictions::table
        .filter(restrictions::kind.eq("ban"))
        .filter(restrictions::scope.eq("site"))
        .filter(restrictions::ends_at.is_null())
        .filter(restrictions::lifted_at.is_null())
        .filter(restrictions::starts_at.le(&now))
        .select(restrictions::user_id)
        .load(conn)?;
    ids.extend(banned);
    Ok(ids)
}

/// Refuses sign-in and refresh for a deactivated or banned account, with a
/// body that says why and until when.
pub fn sign_in_refusal(conn: &mut Conn, user_id: &str) -> ApiResult<()> {
    let deactivated: Option<Option<String>> = users::table
        .find(user_id)
        .select(users::deactivated_at)
        .first(conn)
        .optional()?;
    if matches!(deactivated, Some(Some(_))) {
        return Err(deactivated_error());
    }
    let rows = active(conn, user_id)?;
    match site_ban(&rows) {
        Some(ban) => Err(banned(ban)),
        None => Ok(()),
    }
}

/// `can` for the account, loading what is in force. For the handful of
/// places that need an answer in the middle of a handler.
pub fn check(conn: &mut Conn, user_id: &str, capability: Capability) -> ApiResult<()> {
    can(&active(conn, user_id)?, capability)
}

// --- the table ---------------------------------------------------------------

/// What a route needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Never restricted: signing in and out, the account's own security,
    /// exporting, appealing, and the staff routes (which check the role).
    Open,
    Needs(Capability),
}

use Capability::*;
use Gate::*;

/// Every mutating route, and what it needs. Methods are lower case, paths are
/// the OpenAPI templates. A route missing from here fails
/// `every_write_route_is_gated`.
pub const GATES: &[(&str, &str, Gate)] = &[
    // --- never restricted
    ("post", "/api/me/verification", Open),
    ("post", "/api/auth/verify", Open),
    ("post", "/api/auth/password/forgot", Open),
    ("post", "/api/auth/password/reset", Open),
    ("post", "/api/auth/setup", Open),
    ("patch", "/api/me/settings", Open),
    ("post", "/api/me/password", Open),
    ("post", "/api/me/two-factor/setup", Open),
    ("post", "/api/me/two-factor/enable", Open),
    ("post", "/api/me/two-factor/disable", Open),
    ("delete", "/api/me/sessions/{session_id}", Open),
    ("post", "/api/me/sessions/sign-out-others", Open),
    ("post", "/api/auth/register", Open),
    ("post", "/api/auth/login", Open),
    ("post", "/api/auth/refresh", Open),
    ("post", "/api/auth/logout", Open),
    ("post", "/api/me/terms", Open),
    ("delete", "/api/me", Open),
    ("post", "/api/inbox/{item_id}/read", Open),
    ("post", "/api/inbox/digests/{day}/read", Open),
    ("post", "/api/inbox/read-all", Open),
    ("post", "/api/prompts/unsubscribe", Open),
    ("post", "/api/live/ticket", Open),
    // Exports of your own work are always allowed: a read-only writer has 30
    // days to take their work away.
    ("post", "/api/versions/{version_id}/export", Open),
    ("post", "/api/works/{work_id}/export", Open),
    ("post", "/api/works/{work_id}/export/preflight", Open),
    ("post", "/api/versions/{version_id}/export/preflight", Open),
    ("post", "/api/works/{work_id}/export/bundle", Open),
    ("post", "/api/versions/{version_id}/export/bundle", Open),
    ("post", "/api/works/{work_id}/export/preview", Open),
    ("post", "/api/versions/{version_id}/export/preview", Open),
    ("post", "/api/works/{work_id}/export/again", Open),
    ("post", "/api/ban/export", Open),
    ("post", "/api/ban/appeal", Open),
    ("post", "/api/me/restrictions/{restriction_id}/appeal", Open),
    // Reading position is a side effect of reading.
    ("put", "/api/publications/{publication_id}/progress", Open),
    // Staff routes check the staff role themselves.
    ("post", "/api/admin/prompts", Open),
    ("patch", "/api/admin/prompts/{prompt_id}", Open),
    ("delete", "/api/admin/prompts/{prompt_id}", Open),
    ("post", "/api/admin/prompts/reorder", Open),
    ("post", "/api/admin/users", Open),
    ("patch", "/api/admin/users/{user_id}", Open),
    ("delete", "/api/admin/users/{user_id}", Open),
    ("post", "/api/admin/users/{user_id}/password-reset", Open),
    ("post", "/api/admin/users/{user_id}/setup-email", Open),
    (
        "post",
        "/api/admin/users/{user_id}/sessions/revoke-all",
        Open,
    ),
    (
        "delete",
        "/api/admin/users/{user_id}/sessions/{session_id}",
        Open,
    ),
    ("post", "/api/admin/users/{user_id}/deactivate", Open),
    ("post", "/api/admin/users/{user_id}/reactivate", Open),
    ("post", "/api/admin/users/{user_id}/delete-content", Open),
    ("post", "/api/admin/users/{user_id}/notes", Open),
    ("post", "/api/admin/users/{user_id}/moderator", Open),
    ("post", "/api/admin/users/{user_id}/restrictions", Open),
    ("patch", "/api/admin/restrictions/{restriction_id}", Open),
    (
        "post",
        "/api/admin/restrictions/{restriction_id}/lift",
        Open,
    ),
    ("post", "/api/admin/appeals/{appeal_id}/decide", Open),
    ("post", "/api/admin/permanent-bans", Open),
    ("post", "/api/admin/permanent-bans/{ban_id}/remove", Open),
    ("post", "/api/admin/blocked-domains", Open),
    ("delete", "/api/admin/blocked-domains/{domain}", Open),
    ("post", "/api/admin/queue/{item_id}/claim", Open),
    ("post", "/api/admin/queue/{item_id}/dismiss", Open),
    ("post", "/api/admin/queue/{item_id}/remove", Open),
    ("post", "/api/admin/queue/{item_id}/restore", Open),
    ("post", "/api/admin/queue/{item_id}/hold", Open),
    ("post", "/api/admin/queue/{item_id}/release", Open),
    ("post", "/api/admin/queue/dismiss", Open),
    ("post", "/api/admin/words", Open),
    ("patch", "/api/admin/words/{term_id}", Open),
    ("delete", "/api/admin/words/{term_id}", Open),
    ("post", "/api/admin/words/test", Open),
    ("post", "/api/admin/words/import", Open),
    ("post", "/api/admin/words/rescan", Open),
    ("post", "/api/admin/words/allow", Open),
    ("delete", "/api/admin/words/allow/{word}", Open),
    ("put", "/api/admin/external", Open),
    ("post", "/api/admin/external/test", Open),
    ("put", "/api/admin/moderation-settings", Open),
    // The platform's webhook carries no user; it is signed instead.
    ("post", "/api/webhooks/moderation/{provider}", Open),
    // --- write: your own work, lists and settings
    ("post", "/api/works/{work_id}/annotations", Needs(Write)),
    (
        "post",
        "/api/publications/{publication_id}/annotations",
        Needs(Write),
    ),
    ("patch", "/api/annotations/{annotation_id}", Needs(Write)),
    ("delete", "/api/annotations/{annotation_id}", Needs(Write)),
    ("post", "/api/me/assistant", Needs(Write)),
    ("delete", "/api/me/assistant", Needs(Write)),
    ("post", "/api/scenes/{scene_id}/assist", Needs(Write)),
    ("patch", "/api/works/{work_id}/metadata", Needs(Write)),
    ("put", "/api/works/{work_id}/cover", Needs(Write)),
    ("delete", "/api/works/{work_id}/cover", Needs(Write)),
    ("post", "/api/scenes/{scene_id}/changes", Needs(Write)),
    ("post", "/api/changes/{change_id}/accept", Needs(Write)),
    ("post", "/api/changes/{change_id}/reject", Needs(Write)),
    (
        "post",
        "/api/scenes/{scene_id}/changes/resolve",
        Needs(Write),
    ),
    ("post", "/api/works/{work_id}/collaborators", Needs(Write)),
    (
        "patch",
        "/api/works/{work_id}/collaborators/{collaborator_id}",
        Needs(Write),
    ),
    (
        "delete",
        "/api/works/{work_id}/collaborators/{collaborator_id}",
        Needs(Write),
    ),
    (
        "post",
        "/api/publications/{publication_id}/comments/state",
        Needs(Write),
    ),
    ("delete", "/api/comments/{comment_id}", Needs(Write)),
    (
        "delete",
        "/api/prompt-critiques/{critique_id}",
        Needs(Write),
    ),
    ("post", "/api/export/profiles", Needs(Write)),
    ("post", "/api/export/profiles/import", Needs(Write)),
    ("patch", "/api/export/profiles/{profile_id}", Needs(Write)),
    ("delete", "/api/export/profiles/{profile_id}", Needs(Write)),
    (
        "post",
        "/api/publications/{publication_id}/questions",
        Needs(Write),
    ),
    ("patch", "/api/questions/{question_id}", Needs(Write)),
    ("delete", "/api/questions/{question_id}", Needs(Write)),
    ("patch", "/api/me/goals", Needs(Write)),
    ("post", "/api/me/blocks", Needs(Write)),
    ("delete", "/api/me/blocks/{user_id}", Needs(Write)),
    ("post", "/api/works/{work_id}/notebook", Needs(Write)),
    ("post", "/api/scenes/{scene_id}/duplicate", Needs(Write)),
    ("post", "/api/works/{work_id}/duplicate", Needs(Write)),
    (
        "post",
        "/api/works/{work_id}/notebook/reorder",
        Needs(Write),
    ),
    ("patch", "/api/notebook/{entry_id}", Needs(Write)),
    ("delete", "/api/notebook/{entry_id}", Needs(Write)),
    ("post", "/api/notebook/{entry_id}/links", Needs(Write)),
    (
        "delete",
        "/api/notebook/{entry_id}/links/{scene_id}",
        Needs(Write),
    ),
    ("patch", "/api/me/profile", Needs(Write)),
    ("post", "/api/works/import", Needs(Write)),
    ("post", "/api/works/{work_id}/import", Needs(Write)),
    ("post", "/api/prompts/enrollment", Needs(Write)),
    ("patch", "/api/prompts/enrollment", Needs(Write)),
    ("delete", "/api/prompts/enrollment", Needs(Write)),
    ("put", "/api/prompts/{day}/entry", Needs(Write)),
    ("post", "/api/prompts/{day}/entry/withdraw", Needs(Write)),
    ("delete", "/api/prompts/{day}/entry", Needs(Write)),
    (
        "post",
        "/api/prompt-entries/{entry_id}/to-work/preview",
        Needs(Write),
    ),
    (
        "post",
        "/api/prompt-entries/{entry_id}/to-work",
        Needs(Write),
    ),
    ("delete", "/api/publications/{publication_id}", Needs(Write)),
    (
        "post",
        "/api/publications/{publication_id}/invites",
        Needs(Write),
    ),
    (
        "delete",
        "/api/publications/{publication_id}/invites/{email}",
        Needs(Write),
    ),
    ("post", "/api/circles", Needs(Write)),
    ("post", "/api/circles/{circle_id}/members", Needs(Write)),
    (
        "delete",
        "/api/circles/{circle_id}/members/{email}",
        Needs(Write),
    ),
    ("delete", "/api/circles/{circle_id}", Needs(Write)),
    ("post", "/api/chapters/{chapter_id}/scenes", Needs(Write)),
    ("patch", "/api/scenes/{scene_id}", Needs(Write)),
    ("put", "/api/scenes/{scene_id}/content", Needs(Write)),
    ("delete", "/api/scenes/{scene_id}", Needs(Write)),
    (
        "post",
        "/api/chapters/{chapter_id}/scenes/reorder",
        Needs(Write),
    ),
    (
        "post",
        "/api/scenes/{scene_id}/revisions/{revision_id}/restore",
        Needs(Write),
    ),
    ("post", "/api/scenes/{scene_id}/split", Needs(Write)),
    ("post", "/api/scenes/{scene_id}/merge", Needs(Write)),
    (
        "post",
        "/api/publications/{publication_id}/links",
        Needs(Write),
    ),
    (
        "post",
        "/api/publications/{publication_id}/links/revoke",
        Needs(Write),
    ),
    ("post", "/api/me/summaries", Needs(Write)),
    ("delete", "/api/me/summaries", Needs(Write)),
    ("post", "/api/scenes/{scene_id}/summary", Needs(Write)),
    ("post", "/api/chapters/{chapter_id}/summary", Needs(Write)),
    ("post", "/api/works/{work_id}/summary", Needs(Write)),
    ("post", "/api/works/{work_id}/tasks", Needs(Write)),
    ("post", "/api/comments/{comment_id}/task", Needs(Write)),
    ("patch", "/api/tasks/{task_id}", Needs(Write)),
    ("delete", "/api/tasks/{task_id}", Needs(Write)),
    ("post", "/api/works/{work_id}/versions", Needs(Write)),
    ("patch", "/api/versions/{version_id}", Needs(Write)),
    ("post", "/api/versions/{version_id}/tags", Needs(Write)),
    (
        "delete",
        "/api/versions/{version_id}/tags/{tag}",
        Needs(Write),
    ),
    ("post", "/api/versions/{version_id}/pin", Needs(Write)),
    ("post", "/api/versions/{version_id}/restore", Needs(Write)),
    ("put", "/api/me/dictionary/{word}", Needs(Write)),
    ("delete", "/api/me/dictionary/{word}", Needs(Write)),
    ("post", "/api/works", Needs(Write)),
    ("patch", "/api/works/{work_id}", Needs(Write)),
    ("delete", "/api/works/{work_id}", Needs(Write)),
    ("put", "/api/works/{work_id}/place", Needs(Write)),
    ("post", "/api/works/{work_id}/chapters", Needs(Write)),
    ("patch", "/api/chapters/{chapter_id}", Needs(Write)),
    ("delete", "/api/chapters/{chapter_id}", Needs(Write)),
    (
        "post",
        "/api/works/{work_id}/chapters/reorder",
        Needs(Write),
    ),
    // --- publish: making something public
    ("post", "/api/versions/{version_id}/publish", Needs(Publish)),
    (
        "post",
        "/api/versions/{version_id}/finalize",
        Needs(Publish),
    ),
    (
        "post",
        "/api/versions/{version_id}/publications",
        Needs(Publish),
    ),
    (
        "patch",
        "/api/publications/{publication_id}",
        Needs(Publish),
    ),
    // --- discuss: comments, votes, reactions, answers, submissions, critiques
    (
        "post",
        "/api/publications/{publication_id}/comments",
        Needs(Discuss),
    ),
    // Editing the words needs Discuss, checked in the handler; resolving a
    // note is the work's author and is not a discussion.
    ("patch", "/api/comments/{comment_id}", Open),
    ("post", "/api/comments/{comment_id}/vote", Needs(Discuss)),
    (
        "put",
        "/api/publications/{publication_id}/reactions",
        Needs(Discuss),
    ),
    (
        "put",
        "/api/questions/{question_id}/answers",
        Needs(Discuss),
    ),
    ("post", "/api/prompts/{day}/entry/submit", Needs(Discuss)),
    (
        "post",
        "/api/prompt-entries/{entry_id}/critiques",
        Needs(Discuss),
    ),
    (
        "patch",
        "/api/prompt-critiques/{critique_id}",
        Needs(Discuss),
    ),
    // --- report, invite, follow
    ("post", "/api/comments/{comment_id}/report", Needs(Report)),
    (
        "post",
        "/api/prompt-critiques/{critique_id}/report",
        Needs(Report),
    ),
    ("post", "/api/reports", Needs(Report)),
    ("post", "/api/invites", Needs(Invite)),
    ("put", "/api/users/{user_id}/follow", Needs(Follow)),
    ("delete", "/api/users/{user_id}/follow", Needs(Follow)),
];

pub fn gate_for(method: &str, path: &str) -> Option<Gate> {
    let method = method.to_ascii_lowercase();
    GATES
        .iter()
        .find(|(m, p, _)| *m == method && *p == path)
        .map(|(_, _, g)| *g)
}

/// The middleware: refuses a restricted account's request before the handler
/// runs. Requests with no valid token pass through; the handler's own
/// extractor turns them away.
pub async fn enforce(State(state): State<AppState>, request: Request, next: Next) -> Response {
    use axum::http::Method;
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return next.run(request).await;
    }
    let Some(path) = request
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_string())
    else {
        return next.run(request).await;
    };
    let gate = gate_for(request.method().as_str(), &path);
    let Some(user_id) = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .and_then(|t| crate::auth::jwt::verify(&state.config.jwt_secret, t).ok())
        .map(|c| c.sub)
    else {
        return next.run(request).await;
    };
    let capability = match gate {
        Some(Open) => return next.run(request).await,
        Some(Needs(c)) => c,
        // A route nobody classified is treated as a write, so forgetting it
        // fails closed; the test fails the build as well.
        None => Write,
    };
    let checked = blocking(&state.pool, move |conn| {
        let rows = active(conn, &user_id)?;
        can(&rows, capability)
    })
    .await;
    match checked {
        Ok(()) => next.run(request).await,
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(kind: &str, scope: &str, ends: Option<&str>) -> Restriction {
        Restriction {
            id: "r".into(),
            user_id: "u".into(),
            kind: kind.into(),
            scope: scope.into(),
            starts_at: "2026-01-01T00:00:00.000Z".into(),
            ends_at: ends.map(str::to_string),
            reason_code: "3.1".into(),
            reason_text: "because".into(),
            internal_note: None,
            created_by: "m".into(),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            lifted_at: None,
            lifted_by: None,
            lift_reason: None,
            ended_notified_at: None,
            linked_items: None,
        }
    }

    fn allowed(rows: &[Restriction]) -> Vec<bool> {
        [Write, Publish, Discuss, Report, Invite, Follow]
            .into_iter()
            .map(|c| can(rows, c).is_ok())
            .collect()
    }

    /// The table in the ticket, row by row.
    #[test]
    fn each_kind_and_scope_takes_away_what_the_table_says() {
        // write, publish, discuss, report, invite, follow
        assert_eq!(allowed(&[]), [true; 6]);
        assert_eq!(allowed(&[r("warning", "site", None)]), [true; 6]);
        assert_eq!(
            allowed(&[r("suspend", "site", Some("2099"))]),
            [true, false, false, false, false, false]
        );
        assert_eq!(allowed(&[r("read_only", "site", Some("2099"))]), [false; 6]);
        assert_eq!(allowed(&[r("ban", "site", None)]), [false; 6]);
        for kind in ["ban", "suspend", "read_only"] {
            assert_eq!(
                allowed(&[r(kind, "discussion", Some("2099"))]),
                [true, true, false, false, false, false],
                "{kind} in discussions"
            );
        }
    }

    #[test]
    fn a_restriction_stops_applying_at_its_end_without_a_job() {
        let row = r("suspend", "site", Some("2026-06-01T00:00:00.000Z"));
        assert!(in_force(&row, "2026-05-31T23:59:59.999Z"));
        assert!(!in_force(&row, "2026-06-01T00:00:00.000Z"));
        let mut lifted = r("ban", "site", None);
        assert!(in_force(&lifted, "2030-01-01T00:00:00.000Z"));
        lifted.lifted_at = Some("2026-02-01T00:00:00.000Z".into());
        assert!(!in_force(&lifted, "2030-01-01T00:00:00.000Z"));
    }

    #[test]
    fn only_read_only_and_permanent_site_bans_close_an_account() {
        assert!(is_closing(&r("read_only", "site", Some("x"))));
        assert!(is_closing(&r("ban", "site", None)));
        assert!(!is_closing(&r("ban", "site", Some("x"))));
        assert!(!is_closing(&r("ban", "discussion", None)));
        assert!(!is_closing(&r("suspend", "site", None)));
        assert!(!is_closing(&r("read_only", "discussion", None)));
    }

    #[test]
    fn the_refusal_names_the_capability_and_the_end() {
        let rows = [r("suspend", "discussion", Some("2099-01-01T00:00:00.000Z"))];
        match can(&rows, Discuss) {
            Err(ApiError::Coded { code, details, .. }) => {
                assert_eq!(code, "account_restricted");
                assert_eq!(details["capability"], "discuss");
                assert_eq!(details["until"], "2099-01-01T00:00:00.000Z");
                assert_eq!(details["reason"], "because");
            }
            other => panic!("expected a restriction, got {other:?}"),
        }
    }

    #[test]
    fn every_write_route_is_gated() {
        let doc = crate::routes::openapi_document();
        let mut seen = std::collections::HashSet::new();
        for (path, item) in &doc.paths.paths {
            let methods = [
                ("post", item.post.is_some()),
                ("put", item.put.is_some()),
                ("patch", item.patch.is_some()),
                ("delete", item.delete.is_some()),
            ];
            for (method, present) in methods {
                if !present {
                    continue;
                }
                seen.insert((method.to_string(), path.clone()));
                assert!(
                    gate_for(method, path).is_some(),
                    "{method} {path} is a write route with no entry in restrictions::GATES"
                );
            }
        }
        // And no entry for a route that is not there, so the table cannot rot.
        for (method, path, _) in GATES {
            assert!(
                seen.contains(&(method.to_string(), path.to_string())),
                "GATES names {method} {path}, which is not a route"
            );
        }
    }
}
