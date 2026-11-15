//! Cleaning up after a run of scripted sign-ups: where recent sign-ups came
//! from, and deactivating every account that matches an address, a domain and
//! a window in one go.
//!
//! Deactivating, not deleting: it signs the accounts out and keeps them out,
//! and an account caught by a filter drawn too wide can be reactivated from
//! its page. Deleting stays one account at a time, with its own confirmation.
use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Duration, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::{Permission, StaffUser};
use crate::clock;
use crate::db::schema::{signup_events, users};
use crate::db::{blocking, Conn};
use crate::domain::enums::AuditAction;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::account::revoke_sessions;
use super::audit::record_staff;
use super::privacy::FORMER_MEMBER;

/// Groups shown in each list of the summary.
const TOP: i64 = 25;
/// Accounts shown from a bulk match.
const SAMPLE: i64 = 100;

fn iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct SummaryQuery {
    /// How far back to look, in hours. 24 unless said; at most 720 (30 days).
    pub hours: Option<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignupGroup {
    /// The address (an IPv6 address as its /64) or the email domain.
    pub key: String,
    pub total: i64,
    /// How many of them have not proved their email address.
    pub unverified: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SignupSummary {
    pub hours: i64,
    /// Accounts made in the window that are not deactivated.
    pub total: i64,
    pub unverified: i64,
    /// The busiest addresses sign-ups came from. Only accounts made since
    /// the sign-up guard began recording have one.
    pub by_ip: Vec<SignupGroup>,
    /// The busiest email domains.
    pub by_domain: Vec<SignupGroup>,
}

fn group(rows: Vec<(String, bool)>) -> Vec<SignupGroup> {
    let mut groups: Vec<SignupGroup> = Vec::new();
    let mut index = std::collections::HashMap::new();
    for (key, verified) in rows {
        let i = *index.entry(key.clone()).or_insert_with(|| {
            groups.push(SignupGroup {
                key,
                total: 0,
                unverified: 0,
            });
            groups.len() - 1
        });
        groups[i].total += 1;
        if !verified {
            groups[i].unverified += 1;
        }
    }
    groups.sort_by(|a, b| b.total.cmp(&a.total).then_with(|| a.key.cmp(&b.key)));
    groups.truncate(TOP as usize);
    groups
}

#[utoipa::path(
    get,
    path = "/api/admin/signups",
    tag = "admin",
    params(SummaryQuery),
    responses(
        (status = 200, description = "Where recent sign-ups came from", body = SignupSummary),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn summary(
    State(state): State<AppState>,
    staff: StaffUser,
    Query(query): Query<SummaryQuery>,
) -> ApiResult<Json<SignupSummary>> {
    // Addresses and IPs: the same permission that shows them on an account.
    staff.require(Permission::ViewAccountPii)?;
    let hours = query.hours.unwrap_or(24).clamp(1, 720);
    let since = iso(clock::now() - Duration::hours(hours));
    let summary = blocking(&state.pool, move |conn| {
        let live = users::table
            .filter(users::created_at.ge(&since))
            .filter(users::deactivated_at.is_null())
            .filter(users::id.ne(FORMER_MEMBER));
        // One row per account. A busy day is thousands, which is nothing to
        // group here and keeps the domain arithmetic out of SQL.
        let accounts: Vec<(String, Option<String>)> = live
            .select((users::email, users::email_verified_at))
            .load(conn)?;
        let total = accounts.len() as i64;
        let unverified = accounts.iter().filter(|(_, v)| v.is_none()).count() as i64;
        let by_domain = group(
            accounts
                .into_iter()
                .map(|(email, v)| (crate::domain::bans::domain_of(&email), v.is_some()))
                .collect(),
        );
        let by_ip: Vec<(String, Option<String>)> = signup_events::table
            .inner_join(users::table)
            .filter(signup_events::created_at.ge(&since))
            .filter(users::deactivated_at.is_null())
            .select((signup_events::ip, users::email_verified_at))
            .load(conn)?;
        let by_ip = group(by_ip.into_iter().map(|(ip, v)| (ip, v.is_some())).collect());
        Ok(SignupSummary {
            hours,
            total,
            unverified,
            by_ip,
            by_domain,
        })
    })
    .await?;
    Ok(Json(summary))
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkDeactivateRequest {
    /// The address sign-ups came from, as the summary shows it.
    #[serde(default)]
    pub ip: Option<String>,
    /// An email domain, `example.com`.
    #[serde(default)]
    pub email_domain: Option<String>,
    /// Made at or after this instant (RFC 3339).
    #[serde(default)]
    pub created_after: Option<String>,
    /// Made before this instant (RFC 3339).
    #[serde(default)]
    pub created_before: Option<String>,
    /// Accounts that proved their address too. Off unless asked: a script
    /// rarely proves one, and a person caught by a wide filter often has.
    #[serde(default)]
    pub include_verified: bool,
    /// Only say what would be deactivated. On unless turned off, so that
    /// acting is always a second, deliberate request.
    #[serde(default = "yes")]
    pub dry_run: bool,
    /// Why. Required to act; it goes in the log against every account.
    #[serde(default)]
    pub reason: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MatchedAccount {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub created_at: String,
    pub email_verified: bool,
    pub signup_ip: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkDeactivateResponse {
    pub dry_run: bool,
    /// Accounts the filter matches.
    pub matched: i64,
    /// Accounts this request deactivated; 0 on a dry run.
    pub deactivated: usize,
    /// Up to 100 of the matched accounts, newest first.
    pub sample: Vec<MatchedAccount>,
}

/// The filter, checked and in the form the rows are stored in.
#[derive(Debug, Clone)]
struct Filter {
    ip: Option<String>,
    domain: Option<String>,
    after: Option<String>,
    before: Option<String>,
    include_verified: bool,
}

impl Filter {
    fn from_request(body: &BulkDeactivateRequest) -> ApiResult<Self> {
        let text = |v: &Option<String>| {
            v.as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };
        let instant = |v: &Option<String>, name: &str| -> ApiResult<Option<String>> {
            text(v)
                .map(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|t| iso(t.with_timezone(&Utc)))
                        .map_err(|_| {
                            ApiError::BadRequest(format!(
                                "{name} must be a date and time, RFC 3339"
                            ))
                        })
                })
                .transpose()
        };
        let filter = Filter {
            ip: text(&body.ip),
            domain: text(&body.email_domain)
                .map(|d| d.trim_start_matches('@').to_ascii_lowercase()),
            after: instant(&body.created_after, "createdAfter")?,
            before: instant(&body.created_before, "createdBefore")?,
            include_verified: body.include_verified,
        };
        if filter.ip.is_none() && filter.domain.is_none() && filter.after.is_none() {
            return Err(ApiError::BadRequest(
                "give at least an address, an email domain or a start time".into(),
            ));
        }
        Ok(filter)
    }

    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ip) = &self.ip {
            parts.push(format!("from {ip}"));
        }
        if let Some(d) = &self.domain {
            parts.push(format!("at {d}"));
        }
        if let Some(a) = &self.after {
            parts.push(format!("made from {a}"));
        }
        if let Some(b) = &self.before {
            parts.push(format!("made before {b}"));
        }
        if self.include_verified {
            parts.push("verified addresses too".into());
        }
        format!("in bulk: {}", parts.join(", "))
    }
}

/// The ids of every live account the filter matches, newest first. Staff, the
/// person asking and the placeholder that deleted writers' words are kept
/// under are never matched.
fn matching(conn: &mut Conn, filter: &Filter, caller: &str) -> ApiResult<Vec<String>> {
    let mut q = users::table
        .filter(users::deactivated_at.is_null())
        .filter(users::is_admin.eq(false))
        .filter(users::is_moderator.eq(false))
        .filter(users::id.ne(caller))
        .filter(users::id.ne(FORMER_MEMBER))
        .into_boxed();
    if !filter.include_verified {
        q = q.filter(users::email_verified_at.is_null());
    }
    if let Some(ip) = &filter.ip {
        q = q.filter(
            users::id.nullable().eq_any(
                signup_events::table
                    .filter(signup_events::ip.eq(ip.clone()))
                    .select(signup_events::user_id),
            ),
        );
    }
    if let Some(domain) = &filter.domain {
        let escaped = domain
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        q = q.filter(users::email.like(format!("%@{escaped}")).escape('\\'));
    }
    if let Some(after) = &filter.after {
        q = q.filter(users::created_at.ge(after.clone()));
    }
    if let Some(before) = &filter.before {
        q = q.filter(users::created_at.lt(before.clone()));
    }
    Ok(q.order((users::created_at.desc(), users::id))
        .select(users::id)
        .load(conn)?)
}

fn sample(conn: &mut Conn, ids: &[String]) -> ApiResult<Vec<MatchedAccount>> {
    let ids = &ids[..ids.len().min(SAMPLE as usize)];
    let rows: Vec<(String, String, String, String, Option<String>)> = users::table
        .filter(users::id.eq_any(ids))
        .select((
            users::id,
            users::email,
            users::display_name,
            users::created_at,
            users::email_verified_at,
        ))
        .load(conn)?;
    let ips: std::collections::HashMap<String, String> = signup_events::table
        .filter(signup_events::user_id.eq_any(ids))
        .select((signup_events::user_id.assume_not_null(), signup_events::ip))
        .load::<(String, String)>(conn)?
        .into_iter()
        .collect();
    let mut accounts: Vec<MatchedAccount> = rows
        .into_iter()
        .map(
            |(id, email, display_name, created_at, verified)| MatchedAccount {
                signup_ip: ips.get(&id).cloned(),
                id,
                email,
                display_name,
                created_at,
                email_verified: verified.is_some(),
            },
        )
        .collect();
    accounts.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(accounts)
}

#[utoipa::path(
    post,
    path = "/api/admin/users/bulk-deactivate",
    tag = "admin",
    request_body = BulkDeactivateRequest,
    responses(
        (status = 200, description = "Matched, and unless a dry run, deactivated", body = BulkDeactivateResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn bulk_deactivate(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<BulkDeactivateRequest>,
) -> ApiResult<Json<BulkDeactivateResponse>> {
    staff.require(Permission::ManageAccounts)?;
    staff.require(Permission::ViewAccountPii)?;
    let filter = Filter::from_request(&body)?;
    let reason = if body.dry_run {
        None
    } else {
        let reason = body.reason.as_deref().unwrap_or("").trim().to_string();
        if reason.is_empty() {
            return Err(ApiError::BadRequest("a reason is required".into()));
        }
        if reason.chars().count() > 1_000 {
            return Err(ApiError::BadRequest(
                "keep the reason under 1,000 characters".into(),
            ));
        }
        Some(reason)
    };
    let dry_run = body.dry_run;
    let (matched, deactivated, sample) = blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            let ids = matching(conn, &filter, staff.id())?;
            let shown = sample(conn, &ids)?;
            let Some(reason) = reason else {
                return Ok((ids.len() as i64, 0, shown));
            };
            let now = clock::now_iso();
            let detail = filter.describe();
            for id in &ids {
                diesel::update(users::table.find(id))
                    .set(users::deactivated_at.eq(Some(&now)))
                    .execute(conn)?;
                revoke_sessions(conn, id, None)?;
                record_staff(
                    conn,
                    staff.id(),
                    Some(id),
                    AuditAction::AccountDeactivated,
                    &detail,
                    Some(&reason),
                )?;
            }
            tracing::warn!(staff = %staff.id(), count = ids.len(), %detail, "accounts deactivated in bulk");
            Ok((ids.len() as i64, ids.len(), shown))
        })
    })
    .await?;
    Ok(Json(BulkDeactivateResponse {
        dry_run,
        matched,
        deactivated,
        sample,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_needs_something_to_narrow_it_and_dates_must_parse() {
        let body =
            |v: serde_json::Value| -> BulkDeactivateRequest { serde_json::from_value(v).unwrap() };
        assert!(Filter::from_request(&body(serde_json::json!({}))).is_err());
        assert!(Filter::from_request(&body(
            serde_json::json!({ "createdBefore": "2026-11-01T00:00:00Z" })
        ))
        .is_err());
        assert!(
            Filter::from_request(&body(serde_json::json!({ "createdAfter": "yesterday" })))
                .is_err()
        );
        let f = Filter::from_request(&body(serde_json::json!({
            "emailDomain": " @Spam.Example ",
            "createdAfter": "2026-11-01T00:00:00+01:00",
        })))
        .unwrap();
        assert_eq!(f.domain.as_deref(), Some("spam.example"));
        assert_eq!(f.after.as_deref(), Some("2026-10-31T23:00:00.000Z"));
        assert!(
            body(serde_json::json!({ "ip": "1.2.3.4" })).dry_run,
            "a dry run unless said"
        );
    }
}
