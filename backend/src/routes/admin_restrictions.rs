//! Restricting an account: setting, changing and lifting a restriction, the
//! appeals staff decide, and the permanent ban list.
//!
//! The model and its enforcement are in `domain::restrictions`. This is the
//! staff side of it. Everything here is recorded in the moderation log with a
//! reason, and tells the writer (email and inbox) in the same transaction.
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::Duration;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::{Permission, StaffUser};
use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::bans;
use crate::domain::enums::AuditAction;
use crate::domain::new_id;
use crate::domain::notices;
use crate::domain::restrictions as rules;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::account::mint;
use super::admin_users::{guard_target, RestrictionView};
use super::audit::record_staff;
use super::support;

fn text(value: &Option<String>, what: &str, max: usize) -> ApiResult<String> {
    let v = value.as_deref().unwrap_or("").trim();
    if v.is_empty() {
        return Err(ApiError::BadRequest(format!("{what} is required")));
    }
    if v.chars().count() > max {
        return Err(ApiError::BadRequest(format!(
            "{what} is at most {max} characters"
        )));
    }
    Ok(v.to_string())
}

fn iso(t: chrono::DateTime<chrono::Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// What the staff member needs to be allowed, given what is being set.
fn permission_for(kind: &str, scope: &str, days: Option<i64>) -> Permission {
    match (kind, scope) {
        ("warning", _) => Permission::RestrictDiscussion,
        (_, "discussion") => Permission::RestrictDiscussion,
        ("read_only", "site") => Permission::CloseAccount,
        ("ban", "site") if days.is_none() => Permission::CloseAccount,
        // A permanent site suspension has no end and no appeal clock; it is
        // for administrators.
        ("suspend", "site") if days.is_none() => Permission::CloseAccount,
        _ => Permission::RestrictSiteTemporary,
    }
}

fn validate(kind: &str, scope: &str, days: Option<i64>) -> ApiResult<()> {
    if !rules::KINDS.contains(&kind) {
        return Err(ApiError::BadRequest(
            "kind must be warning, ban, suspend or read_only".into(),
        ));
    }
    if !rules::SCOPES.contains(&scope) {
        return Err(ApiError::BadRequest(
            "scope must be site or discussion".into(),
        ));
    }
    if !rules::DURATIONS.contains(&days) {
        return Err(ApiError::BadRequest(
            "a restriction lasts 3, 7 or 30 days, or is permanent".into(),
        ));
    }
    Ok(())
}

/// Starts the 30-day clock on an account. A second closing restriction does
/// not move the date later.
pub fn start_closing(
    conn: &mut Conn,
    user_id: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> ApiResult<()> {
    let existing: Option<String> = users::table
        .find(user_id)
        .select(users::purge_at)
        .first(conn)?;
    if existing.is_none() {
        diesel::update(users::table.find(user_id))
            .set((
                users::closing_at.eq(Some(iso(now))),
                users::purge_at.eq(Some(iso(now + Duration::days(rules::CLOSING_DAYS)))),
            ))
            .execute(conn)?;
    }
    Ok(())
}

/// Stops the clock, unless another closing restriction is still in force.
pub fn stop_closing(conn: &mut Conn, user_id: &str) -> ApiResult<()> {
    let still = rules::active(conn, user_id)?.iter().any(rules::is_closing);
    if !still {
        diesel::update(users::table.find(user_id))
            .set((
                users::closing_at.eq(None::<String>),
                users::purge_at.eq(None::<String>),
            ))
            .execute(conn)?;
    }
    Ok(())
}

pub fn restriction(conn: &mut Conn, id: &str) -> ApiResult<Restriction> {
    restrictions::table
        .find(id)
        .select(Restriction::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("no such restriction".into()))
}

fn view(conn: &mut Conn, r: Restriction) -> ApiResult<RestrictionView> {
    let appeal: Option<Appeal> = appeals::table
        .filter(appeals::restriction_id.eq(&r.id))
        .select(Appeal::as_select())
        .first(conn)
        .optional()?;
    let cards = support::author_cards(conn, std::slice::from_ref(&r.created_by))?;
    Ok(RestrictionView {
        in_force: rules::in_force(&r, &clock::now_iso()),
        created_by_name: cards.get(&r.created_by).map(|c| c.display_name.clone()),
        appeal,
        restriction: r,
    })
}

/// Lifts a restriction and everything that followed from it: the deletion
/// clock, the permanent ban list entry. Shared by lifting by hand and by an
/// appeal that is overturned.
pub fn lift(conn: &mut Conn, r: &Restriction, actor: &str, reason: &str) -> ApiResult<()> {
    let now = clock::now_iso();
    diesel::update(restrictions::table.find(&r.id))
        .set((
            restrictions::lifted_at.eq(Some(now.clone())),
            restrictions::lifted_by.eq(Some(actor.to_string())),
            restrictions::lift_reason.eq(Some(reason.to_string())),
        ))
        .execute(conn)?;
    if rules::is_closing(r) {
        stop_closing(conn, &r.user_id)?;
    }
    if r.kind == "ban" && r.scope == "site" && r.ends_at.is_none() {
        let removed = diesel::update(
            permanent_bans::table
                .filter(permanent_bans::restriction_id.eq(&r.id))
                .filter(permanent_bans::removed_at.is_null()),
        )
        .set((
            permanent_bans::removed_at.eq(Some(now)),
            permanent_bans::removed_by.eq(Some(actor.to_string())),
            permanent_bans::removed_reason.eq(Some(format!("restriction lifted: {reason}"))),
        ))
        .execute(conn)?;
        if removed > 0 {
            record_staff(
                conn,
                actor,
                Some(&r.user_id),
                AuditAction::PermanentBanRemoved,
                "the ban was lifted",
                Some(reason),
            )?;
        }
    }
    record_staff(
        conn,
        actor,
        Some(&r.user_id),
        AuditAction::RestrictionLifted,
        &format!("{} ({})", r.kind, r.scope),
        Some(reason),
    )?;
    notices::restriction_lifted(conn, r, reason)
}

// --- setting one -----------------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestrictRequest {
    /// `warning`, `ban`, `suspend` or `read_only`.
    pub kind: String,
    /// `site` or `discussion`.
    pub scope: String,
    /// 3, 7 or 30. Absent: permanent. Ignored for a site-wide read-only
    /// restriction, which is always the 30 days before deletion.
    #[serde(default)]
    pub days: Option<i64>,
    /// A section of the community guidelines.
    #[serde(default)]
    pub reason_code: Option<String>,
    /// The message the writer is shown.
    #[serde(default)]
    pub reason_text: Option<String>,
    #[serde(default)]
    pub internal_note: Option<String>,
    /// Queue items that prompted it.
    #[serde(default)]
    pub item_ids: Vec<String>,
    /// For a restriction that ends in deletion: the account's display name,
    /// typed to confirm.
    #[serde(default)]
    pub confirm_name: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/users/{user_id}/restrictions",
    tag = "admin",
    params(("user_id" = String, Path)),
    request_body = RestrictRequest,
    responses(
        (status = 200, description = "Set; the writer is told", body = RestrictionView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn restrict(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(user_id): Path<String>,
    Json(body): Json<RestrictRequest>,
) -> ApiResult<Json<RestrictionView>> {
    let closing_read_only = body.kind == "read_only" && body.scope == "site";
    // Read-only on the whole site is always the 30 days before deletion and
    // has no other length.
    let days = if closing_read_only { None } else { body.days };
    if closing_read_only && body.days.is_some_and(|d| d != 30) {
        return Err(ApiError::BadRequest(
            "read-only on the whole site always runs 30 days and then deletes the account".into(),
        ));
    }
    validate(&body.kind, &body.scope, days)?;
    staff.require(permission_for(&body.kind, &body.scope, days))?;
    let reason_code = text(&body.reason_code, "the guideline section", 40)?;
    let reason_text = text(&body.reason_text, "the message to the writer", 2_000)?;
    let public_url = state.config.public_url.clone();

    let out = blocking(&state.pool, move |conn| {
        guard_target(conn, &staff, &user_id)?;
        conn.transaction::<_, ApiError, _>(|conn| {
            let user = support::find_user(conn, &user_id)?;
            let closing =
                closing_read_only || (body.kind == "ban" && body.scope == "site" && days.is_none());
            if closing
                && body.confirm_name.as_deref().map(str::trim) != Some(user.display_name.trim())
            {
                return Err(ApiError::BadRequest(
                    "this deletes the account: type its display name to confirm".into(),
                ));
            }
            let now = clock::now();
            let id = new_id();
            let ends_at = match body.kind.as_str() {
                // A warning has no length: it is in the history and the
                // ladder, and takes nothing away.
                "warning" => Some(iso(now)),
                _ => rules::ends_at(now, days),
            };
            diesel::insert_into(restrictions::table)
                .values((
                    restrictions::id.eq(&id),
                    restrictions::user_id.eq(&user_id),
                    restrictions::kind.eq(&body.kind),
                    restrictions::scope.eq(&body.scope),
                    restrictions::starts_at.eq(iso(now)),
                    restrictions::ends_at.eq(&ends_at),
                    restrictions::reason_code.eq(&reason_code),
                    restrictions::reason_text.eq(&reason_text),
                    restrictions::internal_note.eq(body
                        .internal_note
                        .as_ref()
                        .map(|n| n.trim().to_string())
                        .filter(|n| !n.is_empty())),
                    restrictions::created_by.eq(staff.id()),
                    restrictions::created_at.eq(iso(now)),
                    restrictions::linked_items.eq((!body.item_ids.is_empty())
                        .then(|| serde_json::json!(body.item_ids).to_string())),
                ))
                .execute(conn)?;
            let r = restriction(conn, &id)?;

            if body.kind == "ban" && body.scope == "site" {
                // The next request from any device finds out; this makes the
                // sessions themselves go too.
                super::account::revoke_sessions(conn, &user_id, None)?;
            }
            if closing {
                start_closing(conn, &user_id, now)?;
            }
            if closing && body.kind == "ban" {
                // The address and the reason outlive the account, so the
                // address can never sign up again.
                diesel::insert_into(permanent_bans::table)
                    .values((
                        permanent_bans::id.eq(new_id()),
                        permanent_bans::email.eq(&user.email),
                        permanent_bans::email_canonical.eq(bans::canonical(&user.email)),
                        permanent_bans::former_user_id.eq(Some(user.id.clone())),
                        permanent_bans::display_name.eq(&user.display_name),
                        permanent_bans::reason_code.eq(&reason_code),
                        permanent_bans::reason_text.eq(&reason_text),
                        permanent_bans::restriction_id.eq(Some(id.clone())),
                        permanent_bans::banned_by.eq(staff.id()),
                        permanent_bans::created_at.eq(iso(now)),
                    ))
                    .on_conflict(permanent_bans::email_canonical)
                    .do_update()
                    .set((
                        permanent_bans::removed_at.eq(None::<String>),
                        permanent_bans::removed_by.eq(None::<String>),
                        permanent_bans::removed_reason.eq(None::<String>),
                        permanent_bans::restriction_id.eq(Some(id.clone())),
                        permanent_bans::reason_code.eq(&reason_code),
                        permanent_bans::reason_text.eq(&reason_text),
                        permanent_bans::former_user_id.eq(Some(user.id.clone())),
                        permanent_bans::banned_by.eq(staff.id()),
                        permanent_bans::created_at.eq(iso(now)),
                    ))
                    .execute(conn)?;
                record_staff(
                    conn,
                    staff.id(),
                    Some(&user_id),
                    AuditAction::PermanentBanListed,
                    "the address will never register again",
                    Some(&reason_text),
                )?;
            }
            // Items that prompted it are done with.
            for item_id in &body.item_ids {
                if let Ok(item) = crate::domain::moderation::item(conn, item_id) {
                    if item.subject_author_id == user_id {
                        diesel::update(moderation_items::table.find(item_id))
                            .set((
                                moderation_items::state.eq("actioned"),
                                moderation_items::decision
                                    .eq(Some("author_restricted".to_string())),
                                moderation_items::decided_by.eq(Some(staff.id().to_string())),
                                moderation_items::decided_at.eq(Some(iso(now))),
                            ))
                            .execute(conn)?;
                    }
                }
            }
            record_staff(
                conn,
                staff.id(),
                Some(&user_id),
                AuditAction::RestrictionAdded,
                &format!(
                    "{} ({}), {} — guideline {}",
                    body.kind,
                    body.scope,
                    match &ends_at {
                        _ if body.kind == "warning" => "a warning".to_string(),
                        Some(end) => format!("until {end}"),
                        None => "permanent".to_string(),
                    },
                    reason_code
                ),
                Some(&reason_text),
            )?;

            // Telling them. A banned writer can't sign in to appeal, so the
            // email carries a single-use link instead.
            let (appeal_url, export_url) = if body.kind == "ban" && body.scope == "site" {
                let token = mint(conn, &user, &format!("appeal:{id}"), Duration::days(60))?;
                (
                    format!("{public_url}/ban/appeal?token={token}"),
                    closing.then(|| format!("{public_url}/ban/export")),
                )
            } else {
                (format!("{public_url}/standing"), None)
            };
            notices::restriction_set(conn, &public_url, &r, &appeal_url, export_url.as_deref())?;
            view(conn, r)
        })
    })
    .await?;
    Ok(Json(out))
}

// --- changing and lifting ------------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRequest {
    /// The new length in days (3, 7 or 30), or `null` for permanent. Left out,
    /// the length stays.
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<i64>)]
    pub days: Option<Option<i64>>,
    #[serde(default)]
    pub reason_text: Option<String>,
    /// Why it is being changed.
    #[serde(default)]
    pub reason: Option<String>,
}

fn double_option<'de, D>(d: D) -> Result<Option<Option<i64>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<i64>::deserialize(d)?))
}

#[utoipa::path(
    patch,
    path = "/api/admin/restrictions/{restriction_id}",
    tag = "admin",
    params(("restriction_id" = String, Path)),
    request_body = ChangeRequest,
    responses(
        (status = 200, description = "Changed; the writer is told", body = RestrictionView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn change(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(restriction_id): Path<String>,
    Json(body): Json<ChangeRequest>,
) -> ApiResult<Json<RestrictionView>> {
    let reason = text(&body.reason, "a reason", 1_000)?;
    let public_url = state.config.public_url.clone();
    let out = blocking(&state.pool, move |conn| {
        let r = restriction(conn, &restriction_id)?;
        guard_target(conn, &staff, &r.user_id)?;
        if !rules::in_force(&r, &clock::now_iso()) || r.kind == "warning" {
            return Err(ApiError::Conflict(
                "only a restriction that is in force can be changed".into(),
            ));
        }
        let new_days = match body.days {
            Some(d) => {
                validate(&r.kind, &r.scope, d)?;
                if rules::is_closing(&r) {
                    return Err(ApiError::BadRequest(
                        "a restriction that ends in deletion keeps its length".into(),
                    ));
                }
                Some(d)
            }
            None => None,
        };
        // Whoever changes it needs the power the new length would have taken
        // to set.
        let perm_days = match new_days {
            Some(d) => d,
            None => r.ends_at.as_ref().map(|_| 3),
        };
        staff.require(permission_for(&r.kind, &r.scope, perm_days))?;
        conn.transaction::<_, ApiError, _>(|conn| {
            if let Some(d) = new_days {
                let start = chrono::DateTime::parse_from_rfc3339(&r.starts_at)
                    .map(|t| t.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| clock::now());
                diesel::update(restrictions::table.find(&r.id))
                    .set(restrictions::ends_at.eq(rules::ends_at(start, d)))
                    .execute(conn)?;
            }
            if let Some(message) = body
                .reason_text
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty())
            {
                diesel::update(restrictions::table.find(&r.id))
                    .set(restrictions::reason_text.eq(message))
                    .execute(conn)?;
            }
            let changed = restriction(conn, &r.id)?;
            record_staff(
                conn,
                staff.id(),
                Some(&r.user_id),
                AuditAction::RestrictionChanged,
                &format!(
                    "{} ({}): ends {} → {}",
                    r.kind,
                    r.scope,
                    r.ends_at.clone().unwrap_or_else(|| "never".into()),
                    changed.ends_at.clone().unwrap_or_else(|| "never".into())
                ),
                Some(&reason),
            )?;
            let appeal_url = if r.kind == "ban" && r.scope == "site" {
                let user = support::find_user(conn, &r.user_id)?;
                let token = mint(conn, &user, &format!("appeal:{}", r.id), Duration::days(60))?;
                format!("{public_url}/ban/appeal?token={token}")
            } else {
                format!("{public_url}/standing")
            };
            notices::restriction_changed(conn, &changed, &appeal_url)?;
            view(conn, changed)
        })
    })
    .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LiftRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/restrictions/{restriction_id}/lift",
    tag = "admin",
    params(("restriction_id" = String, Path)),
    request_body = LiftRequest,
    responses(
        (status = 200, description = "Lifted; the writer is told", body = RestrictionView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn lift_one(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(restriction_id): Path<String>,
    Json(body): Json<LiftRequest>,
) -> ApiResult<Json<RestrictionView>> {
    let reason = text(&body.reason, "a reason", 1_000)?;
    let out = blocking(&state.pool, move |conn| {
        let r = restriction(conn, &restriction_id)?;
        guard_target(conn, &staff, &r.user_id)?;
        let days = if rules::is_closing(&r) { None } else { Some(3) };
        staff.require(permission_for(&r.kind, &r.scope, days))?;
        if r.lifted_at.is_some() {
            return Err(ApiError::Conflict("it has already been lifted".into()));
        }
        conn.transaction::<_, ApiError, _>(|conn| {
            lift(conn, &r, staff.id(), &reason)?;
            let fresh = restriction(conn, &r.id)?;
            view(conn, fresh)
        })
    })
    .await?;
    Ok(Json(out))
}

// --- appeals, staff side -------------------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppealRow {
    pub appeal: Appeal,
    pub restriction: Restriction,
    pub user_id: String,
    pub display_name: String,
    /// Days until the account is deleted, for an appeal on a closing
    /// restriction. The purge waits for the decision.
    pub days_left: Option<i64>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct AppealQuery {
    /// `open` (default) or `all`.
    #[serde(default)]
    pub state: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/appeals",
    tag = "admin",
    params(AppealQuery),
    responses(
        (status = 200, description = "Appeals, closing ones first", body = Vec<AppealRow>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_appeals(
    State(state): State<AppState>,
    staff: StaffUser,
    Query(query): Query<AppealQuery>,
) -> ApiResult<Json<Vec<AppealRow>>> {
    staff.require(Permission::DecideAppeal)?;
    let all = query.state.as_deref() == Some("all");
    let rows = blocking(&state.pool, move |conn| {
        let mut q = appeals::table
            .inner_join(restrictions::table)
            .inner_join(users::table.on(users::id.eq(restrictions::user_id)))
            .order(appeals::created_at.asc())
            .into_boxed();
        if !all {
            q = q.filter(appeals::state.eq("open"));
        }
        let rows: Vec<(Appeal, Restriction, String, String, Option<String>)> = q
            .select((
                Appeal::as_select(),
                Restriction::as_select(),
                users::id,
                users::display_name,
                users::purge_at,
            ))
            .load(conn)?;
        let now = clock::now();
        let mut out: Vec<AppealRow> = rows
            .into_iter()
            .map(
                |(appeal, restriction, user_id, display_name, purge_at)| AppealRow {
                    days_left: purge_at
                        .filter(|_| rules::is_closing(&restriction))
                        .and_then(|p| chrono::DateTime::parse_from_rfc3339(&p).ok())
                        .map(|p| (p.with_timezone(&chrono::Utc) - now).num_days()),
                    appeal,
                    restriction,
                    user_id,
                    display_name,
                },
            )
            .collect();
        // The ones with a deletion date come first, soonest first.
        out.sort_by_key(|a| (a.days_left.is_none(), a.days_left.unwrap_or(i64::MAX)));
        Ok(out)
    })
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecideRequest {
    /// `upheld` (the restriction stands) or `overturned`.
    pub decision: String,
    /// What the writer is told.
    pub message: Option<String>,
    /// On `overturned`: bring back what was removed alongside it.
    #[serde(default)]
    pub restore_content: bool,
}

#[utoipa::path(
    post,
    path = "/api/admin/appeals/{appeal_id}/decide",
    tag = "admin",
    params(("appeal_id" = String, Path)),
    request_body = DecideRequest,
    responses(
        (status = 200, description = "Decided; the writer is told", body = Appeal),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn decide(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(appeal_id): Path<String>,
    Json(body): Json<DecideRequest>,
) -> ApiResult<Json<Appeal>> {
    staff.require(Permission::DecideAppeal)?;
    if !["upheld", "overturned"].contains(&body.decision.as_str()) {
        return Err(ApiError::BadRequest(
            "decision must be upheld or overturned".into(),
        ));
    }
    let message = text(&body.message, "a message to the writer", 2_000)?;
    let out = blocking(&state.pool, move |conn| {
        let appeal: Appeal = appeals::table
            .find(&appeal_id)
            .select(Appeal::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("no such appeal".into()))?;
        let r = restriction(conn, &appeal.restriction_id)?;
        if appeal.state != "open" {
            return Err(ApiError::Conflict("that appeal has been decided".into()));
        }
        // Whoever set a restriction can't decide the appeal against it.
        if r.created_by == staff.id() {
            return Err(ApiError::Forbidden(
                "you set this restriction, so someone else has to decide the appeal".into(),
            ));
        }
        if rules::is_closing(&r) {
            staff.require(Permission::CloseAccount)?;
        }
        conn.transaction::<_, ApiError, _>(|conn| {
            let now = clock::now_iso();
            diesel::update(appeals::table.find(&appeal.id))
                .set((
                    appeals::state.eq(&body.decision),
                    appeals::decided_by.eq(Some(staff.id().to_string())),
                    appeals::decision_text.eq(Some(message.clone())),
                    appeals::decided_at.eq(Some(now)),
                ))
                .execute(conn)?;
            let overturned = body.decision == "overturned";
            if overturned && r.lifted_at.is_none() {
                lift(conn, &r, staff.id(), "appeal overturned")?;
            }
            if overturned && body.restore_content {
                let linked: Vec<String> = r
                    .linked_items
                    .as_deref()
                    .and_then(|l| serde_json::from_str(l).ok())
                    .unwrap_or_default();
                for item_id in linked {
                    if let Ok(item) = crate::domain::moderation::item(conn, &item_id) {
                        if let Some(s) = crate::domain::moderation::subject(
                            conn,
                            &item.subject_type,
                            &item.subject_id,
                        )? {
                            if s.moderation_state == "removed" {
                                crate::domain::moderation::restore(
                                    conn,
                                    &item_id,
                                    staff.id(),
                                    "appeal overturned",
                                )?;
                            }
                        }
                    }
                }
            }
            record_staff(
                conn,
                staff.id(),
                Some(&r.user_id),
                AuditAction::AppealDecided,
                &format!("{} on the {} ({})", body.decision, r.kind, r.scope),
                Some(&message),
            )?;
            notices::appeal_decided(conn, &r.user_id, &r.id, overturned, &message)?;
            Ok(appeals::table
                .find(&appeal.id)
                .select(Appeal::as_select())
                .first(conn)?)
        })
    })
    .await?;
    Ok(Json(out))
}

// --- the permanent ban list --------------------------------------------------------------------

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct BanQuery {
    /// An address or a name.
    #[serde(default)]
    pub q: Option<String>,
    /// Include rows that have been removed.
    #[serde(default)]
    pub include_removed: Option<bool>,
}

#[utoipa::path(
    get,
    path = "/api/admin/permanent-bans",
    tag = "admin",
    params(BanQuery),
    responses(
        (status = 200, description = "The list", body = Vec<PermanentBan>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_bans(
    State(state): State<AppState>,
    staff: StaffUser,
    Query(query): Query<BanQuery>,
) -> ApiResult<Json<Vec<PermanentBan>>> {
    staff.require(Permission::ManagePermanentBans)?;
    let rows = blocking(&state.pool, move |conn| {
        let mut q = permanent_bans::table
            .order(permanent_bans::created_at.desc())
            .into_boxed();
        if query.include_removed != Some(true) {
            q = q.filter(permanent_bans::removed_at.is_null());
        }
        if let Some(t) = query.q.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            let like = format!("%{}%", t.replace('%', "\\%").replace('_', "\\_"));
            q = q.filter(
                permanent_bans::email
                    .like(like.clone())
                    .escape('\\')
                    .or(permanent_bans::display_name.like(like).escape('\\')),
            );
        }
        Ok(q.select(PermanentBan::as_select()).load(conn)?)
    })
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddBanRequest {
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub reason_code: String,
    pub reason_text: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/permanent-bans",
    tag = "admin",
    request_body = AddBanRequest,
    responses(
        (status = 200, description = "Listed", body = PermanentBan),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn add_ban(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<AddBanRequest>,
) -> ApiResult<Json<PermanentBan>> {
    staff.require(Permission::ManagePermanentBans)?;
    let email = super::auth::normalize_email(&body.email)?;
    let reason_code = text(&Some(body.reason_code.clone()), "the guideline section", 40)?;
    let reason_text = text(&body.reason_text, "a reason", 2_000)?;
    let out = blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            if bans::banned_address(conn, &email)?.is_some() {
                return Err(ApiError::Conflict("that address is already listed".into()));
            }
            let canonical = bans::canonical(&email);
            // A row that was removed earlier is brought back rather than
            // duplicated: the canonical address is unique.
            diesel::delete(
                permanent_bans::table
                    .filter(permanent_bans::email_canonical.eq(&canonical))
                    .filter(permanent_bans::removed_at.is_not_null()),
            )
            .execute(conn)?;
            let id = new_id();
            diesel::insert_into(permanent_bans::table)
                .values((
                    permanent_bans::id.eq(&id),
                    permanent_bans::email.eq(&email),
                    permanent_bans::email_canonical.eq(&canonical),
                    permanent_bans::display_name.eq(body.display_name.clone().unwrap_or_default()),
                    permanent_bans::reason_code.eq(&reason_code),
                    permanent_bans::reason_text.eq(&reason_text),
                    permanent_bans::banned_by.eq(staff.id()),
                    permanent_bans::created_at.eq(clock::now_iso()),
                ))
                .execute(conn)?;
            record_staff(
                conn,
                staff.id(),
                None,
                AuditAction::PermanentBanListed,
                "an address was added by hand",
                Some(&reason_text),
            )?;
            Ok(permanent_bans::table
                .find(&id)
                .select(PermanentBan::as_select())
                .first(conn)?)
        })
    })
    .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemoveBanRequest {
    pub reason: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/permanent-bans/{ban_id}/remove",
    tag = "admin",
    params(("ban_id" = String, Path)),
    request_body = RemoveBanRequest,
    responses(
        (status = 200, description = "Removed; the address can register again", body = PermanentBan),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn remove_ban(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(ban_id): Path<String>,
    Json(body): Json<RemoveBanRequest>,
) -> ApiResult<Json<PermanentBan>> {
    staff.require(Permission::ManagePermanentBans)?;
    let reason = text(&body.reason, "a reason", 1_000)?;
    let out = blocking(&state.pool, move |conn| {
        let done = diesel::update(
            permanent_bans::table
                .find(&ban_id)
                .filter(permanent_bans::removed_at.is_null()),
        )
        .set((
            permanent_bans::removed_at.eq(Some(clock::now_iso())),
            permanent_bans::removed_by.eq(Some(staff.id().to_string())),
            permanent_bans::removed_reason.eq(Some(reason.clone())),
        ))
        .execute(conn)?;
        if done == 0 {
            return Err(ApiError::NotFound("no such listed address".into()));
        }
        let row: PermanentBan = permanent_bans::table
            .find(&ban_id)
            .select(PermanentBan::as_select())
            .first(conn)?;
        record_staff(
            conn,
            staff.id(),
            row.former_user_id.as_deref(),
            AuditAction::PermanentBanRemoved,
            "removed from the list",
            Some(&reason),
        )?;
        Ok(row)
    })
    .await?;
    Ok(Json(out))
}

// --- blocked domains ----------------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/api/admin/blocked-domains",
    tag = "admin",
    responses(
        (status = 200, description = "Throwaway-mail domains", body = Vec<BlockedDomain>),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list_domains(
    State(state): State<AppState>,
    staff: StaffUser,
) -> ApiResult<Json<Vec<BlockedDomain>>> {
    staff.require(Permission::ManagePermanentBans)?;
    let rows = blocking(&state.pool, move |conn| {
        Ok(blocked_domains::table
            .order(blocked_domains::domain.asc())
            .select(BlockedDomain::as_select())
            .load(conn)?)
    })
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DomainRequest {
    pub domain: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/blocked-domains",
    tag = "admin",
    request_body = DomainRequest,
    responses(
        (status = 200, description = "Blocked", body = BlockedDomain),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn add_domain(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<DomainRequest>,
) -> ApiResult<Json<BlockedDomain>> {
    staff.require(Permission::ManagePermanentBans)?;
    let domain = body.domain.trim().trim_start_matches('@').to_lowercase();
    if domain.is_empty() || !domain.contains('.') || domain.contains(char::is_whitespace) {
        return Err(ApiError::BadRequest("that is not a domain".into()));
    }
    let out = blocking(&state.pool, move |conn| {
        let row = BlockedDomain {
            domain: domain.clone(),
            note: body.note.clone().unwrap_or_default(),
            created_by: staff.id().to_string(),
            created_at: clock::now_iso(),
        };
        diesel::insert_into(blocked_domains::table)
            .values((
                blocked_domains::domain.eq(&row.domain),
                blocked_domains::note.eq(&row.note),
                blocked_domains::created_by.eq(&row.created_by),
                blocked_domains::created_at.eq(&row.created_at),
            ))
            .execute(conn)
            .map_err(|_| ApiError::Conflict("that domain is already blocked".into()))?;
        record_staff(
            conn,
            staff.id(),
            None,
            AuditAction::BlockedDomainChanged,
            &format!("blocked {domain}"),
            None,
        )?;
        Ok(row)
    })
    .await?;
    Ok(Json(out))
}

#[utoipa::path(
    delete,
    path = "/api/admin/blocked-domains/{domain}",
    tag = "admin",
    params(("domain" = String, Path)),
    responses(
        (status = 200, description = "Unblocked", body = crate::routes::account::Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn remove_domain(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(domain): Path<String>,
) -> ApiResult<Json<super::account::Done>> {
    staff.require(Permission::ManagePermanentBans)?;
    blocking(&state.pool, move |conn| {
        let done =
            diesel::delete(blocked_domains::table.find(domain.to_lowercase())).execute(conn)?;
        if done == 0 {
            return Err(ApiError::NotFound("that domain is not blocked".into()));
        }
        record_staff(
            conn,
            staff.id(),
            None,
            AuditAction::BlockedDomainChanged,
            &format!("unblocked {domain}"),
            None,
        )
    })
    .await?;
    Ok(Json(super::account::Done { ok: true }))
}
