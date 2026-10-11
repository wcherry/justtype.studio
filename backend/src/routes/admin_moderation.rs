//! The moderation side of the admin area: the queue of flagged content and
//! accounts, the word filter, the external platform's settings, the
//! thresholds, and the log of everything staff did.
use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::IntoResponse;
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::{Permission, StaffUser};
use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::enums::AuditAction;
use crate::domain::external_moderation::{self as external, CheckItem, Recommended};
use crate::domain::filter::{self, FilterAction, Surface};
use crate::domain::moderation::{self as moderation};
use crate::domain::new_id;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::account::Done;
use super::audit::record_staff;
use super::support::{self, AuthorCard};

const PAGE: i64 = 50;
const CLAIM_MINUTES: i64 = 10;

fn reason_of(value: &Option<String>) -> ApiResult<String> {
    let v = value.as_deref().unwrap_or("").trim();
    if v.is_empty() {
        return Err(ApiError::BadRequest("a reason is required".into()));
    }
    if v.chars().count() > 1_000 {
        return Err(ApiError::BadRequest(
            "keep the reason under 1,000 characters".into(),
        ));
    }
    Ok(v.to_string())
}

// --- the queue ---------------------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReportDetail {
    pub reporter: AuthorCard,
    pub reason: String,
    pub detail: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    #[serde(flatten)]
    pub item: ModerationItem,
    /// What was said, as text. Empty for an account.
    pub text: String,
    pub surface: Option<String>,
    /// Where it sits: the publication, the prompt, what it replies to.
    pub context: String,
    /// What moderation has done with it: `visible`, `held` or `removed`.
    pub subject_state: String,
    pub author: AuthorCard,
    /// Administrators only.
    pub author_email: Option<String>,
    pub sources_list: Vec<String>,
    pub reports: Vec<ReportDetail>,
    pub claimed_by_name: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct QueueQuery {
    /// `content` (default), `accounts`, `held` or `done`.
    #[serde(default)]
    pub tab: Option<String>,
    /// `report`, `word_filter`, `external` or `threshold`.
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    /// A subject type: `comment`, `entry`, `critique`, …
    #[serde(default)]
    pub surface: Option<String>,
    /// Only items claimed by me.
    #[serde(default)]
    pub mine: Option<bool>,
    #[serde(default)]
    pub offset: Option<i64>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueuePage {
    pub items: Vec<QueueItem>,
    pub total: i64,
    /// Open items, for the menu badge.
    pub open_count: i64,
}

fn enrich(
    conn: &mut Conn,
    staff: &StaffUser,
    items: Vec<ModerationItem>,
) -> ApiResult<Vec<QueueItem>> {
    let pii = staff.can(Permission::ViewAccountPii);
    let author_ids: Vec<String> = items
        .iter()
        .flat_map(|i| [Some(i.subject_author_id.clone()), i.claimed_by.clone()])
        .flatten()
        .collect();
    let cards = support::author_cards(conn, &author_ids)?;
    let emails: HashMap<String, String> = if pii {
        users::table
            .filter(users::id.eq_any(&author_ids))
            .select((users::id, users::email))
            .load::<(String, String)>(conn)?
            .into_iter()
            .collect()
    } else {
        HashMap::new()
    };
    let mut out = Vec::new();
    for item in items {
        let subject = moderation::subject(conn, &item.subject_type, &item.subject_id)?;
        let mut reports = reporters_of(conn, &item)?;
        reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        let reporter_ids: Vec<String> = reports.iter().map(|r| r.reporter.id.clone()).collect();
        let reporter_cards = support::author_cards(conn, &reporter_ids)?;
        for r in &mut reports {
            if let Some(card) = reporter_cards.get(&r.reporter.id) {
                r.reporter = card.clone();
            }
        }
        let author = cards
            .get(&item.subject_author_id)
            .cloned()
            .unwrap_or(AuthorCard {
                id: item.subject_author_id.clone(),
                display_name: "Deleted account".into(),
            });
        let live_claim = item
            .claimed_until
            .as_deref()
            .is_some_and(|until| until > clock::now_iso().as_str());
        out.push(QueueItem {
            claimed_by_name: item
                .claimed_by
                .as_ref()
                .filter(|_| live_claim)
                .and_then(|c| cards.get(c))
                .map(|c| c.display_name.clone()),
            author_email: emails.get(&item.subject_author_id).cloned(),
            sources_list: serde_json::from_str(&item.sources).unwrap_or_default(),
            text: subject.as_ref().map(|s| s.text.clone()).unwrap_or_default(),
            surface: subject
                .as_ref()
                .and_then(|s| s.surface)
                .map(|s| s.as_str().to_string()),
            context: subject
                .as_ref()
                .map(|s| s.context.clone())
                .unwrap_or_default(),
            subject_state: subject
                .as_ref()
                .map(|s| s.moderation_state.clone())
                .unwrap_or_else(|| "visible".into()),
            author,
            reports,
            item,
        });
    }
    Ok(out)
}

/// Everyone who reported a thing, from whichever table their report is in.
fn reporters_of(conn: &mut Conn, item: &ModerationItem) -> ApiResult<Vec<ReportDetail>> {
    let card = |id: String| AuthorCard {
        id,
        display_name: "Unknown".into(),
    };
    let mut out: Vec<ReportDetail> = reports::table
        .filter(reports::subject_type.eq(&item.subject_type))
        .filter(reports::subject_id.eq(&item.subject_id))
        .select((
            reports::reporter_id,
            reports::reason,
            reports::detail,
            reports::created_at,
        ))
        .load::<(String, String, Option<String>, String)>(conn)?
        .into_iter()
        .map(|(r, reason, detail, at)| ReportDetail {
            reporter: card(r),
            reason,
            detail,
            created_at: at,
        })
        .collect();
    match item.subject_type.as_str() {
        "comment" => out.extend(
            comment_reports::table
                .filter(comment_reports::comment_id.eq(&item.subject_id))
                .select((
                    comment_reports::reporter_id,
                    comment_reports::reason,
                    comment_reports::detail,
                    comment_reports::created_at,
                ))
                .load::<(String, String, Option<String>, String)>(conn)?
                .into_iter()
                .map(|(r, reason, detail, at)| ReportDetail {
                    reporter: card(r),
                    reason,
                    detail,
                    created_at: at,
                }),
        ),
        "critique" => out.extend(
            prompt_critique_reports::table
                .filter(prompt_critique_reports::critique_id.eq(&item.subject_id))
                .select((
                    prompt_critique_reports::reporter_id,
                    prompt_critique_reports::reason,
                    prompt_critique_reports::detail,
                    prompt_critique_reports::created_at,
                ))
                .load::<(String, String, Option<String>, String)>(conn)?
                .into_iter()
                .map(|(r, reason, detail, at)| ReportDetail {
                    reporter: card(r),
                    reason,
                    detail,
                    created_at: at,
                }),
        ),
        _ => {}
    }
    Ok(out)
}

#[utoipa::path(
    get,
    path = "/api/admin/queue",
    tag = "admin",
    params(QueueQuery),
    responses(
        (status = 200, description = "Flagged content and accounts, worst first", body = QueuePage),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn queue(
    State(state): State<AppState>,
    staff: StaffUser,
    Query(query): Query<QueueQuery>,
) -> ApiResult<Json<QueuePage>> {
    staff.require(Permission::ViewQueue)?;
    let tab = query.tab.clone().unwrap_or_else(|| "content".into());
    if !["content", "accounts", "held", "done"].contains(&tab.as_str()) {
        return Err(ApiError::BadRequest(
            "tab must be content, accounts, held or done".into(),
        ));
    }
    let page = blocking(&state.pool, move |conn| {
        let build = |tab: &str| {
            let mut q = moderation_items::table.into_boxed();
            q = match tab {
                "accounts" => q
                    .filter(moderation_items::subject_type.eq("account"))
                    .filter(moderation_items::state.eq("open")),
                "held" => q.filter(moderation_items::state.eq("held")),
                "done" => q.filter(moderation_items::state.eq_any(["actioned", "dismissed"])),
                _ => q
                    .filter(moderation_items::subject_type.ne("account"))
                    .filter(moderation_items::state.eq("open")),
            };
            q
        };
        let mut q = build(&tab);
        if let Some(source) = query.source.as_deref() {
            q = q.filter(moderation_items::sources.like(format!("%\"{source}\"%")));
        }
        if let Some(reason) = query.reason.as_deref() {
            q = q.filter(moderation_items::top_reason.eq(reason.to_string()));
        }
        if let Some(surface) = query.surface.as_deref() {
            q = q.filter(moderation_items::subject_type.eq(surface.to_string()));
        }
        if query.mine == Some(true) {
            q = q
                .filter(moderation_items::claimed_by.eq(staff.id().to_string()))
                .filter(moderation_items::claimed_until.gt(clock::now_iso()));
        }
        let mut counted = build(&tab);
        if let Some(source) = query.source.as_deref() {
            counted = counted.filter(moderation_items::sources.like(format!("%\"{source}\"%")));
        }
        if let Some(reason) = query.reason.as_deref() {
            counted = counted.filter(moderation_items::top_reason.eq(reason.to_string()));
        }
        if let Some(surface) = query.surface.as_deref() {
            counted = counted.filter(moderation_items::subject_type.eq(surface.to_string()));
        }
        let total: i64 = counted.count().get_result(conn)?;
        let rows: Vec<ModerationItem> = q
            .order((
                moderation_items::severity.desc(),
                moderation_items::report_count.desc(),
                moderation_items::first_flagged_at.asc(),
            ))
            .limit(PAGE)
            .offset(query.offset.unwrap_or(0).max(0))
            .select(ModerationItem::as_select())
            .load(conn)?;
        let open_count = moderation_items::table
            .filter(moderation_items::state.eq("open"))
            .count()
            .get_result(conn)?;
        Ok(QueuePage {
            items: enrich(conn, &staff, rows)?,
            total,
            open_count,
        })
    })
    .await?;
    Ok(Json(page))
}

#[utoipa::path(
    post,
    path = "/api/admin/queue/{item_id}/claim",
    tag = "admin",
    params(("item_id" = String, Path)),
    responses(
        (status = 200, description = "Yours for ten minutes", body = QueueItem),
        (status = 409, description = "Someone else has it", body = ErrorEnvelope),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn claim(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(item_id): Path<String>,
) -> ApiResult<Json<QueueItem>> {
    staff.require(Permission::ViewQueue)?;
    let out = blocking(&state.pool, move |conn| {
        let item = moderation::item(conn, &item_id)?;
        let now = clock::now_iso();
        let held_by_other = item.claimed_by.as_deref().is_some_and(|c| c != staff.id())
            && item
                .claimed_until
                .as_deref()
                .is_some_and(|u| u > now.as_str());
        if held_by_other {
            let name = support::author_cards(conn, &[item.claimed_by.clone().unwrap_or_default()])?
                .into_values()
                .next()
                .map(|c| c.display_name)
                .unwrap_or_else(|| "someone".into());
            return Err(ApiError::ConflictWith {
                message: format!("{name} is looking at this"),
                details: json!({ "claimedBy": name, "until": item.claimed_until }),
            });
        }
        let until = (clock::now() + chrono::Duration::minutes(CLAIM_MINUTES))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        diesel::update(moderation_items::table.find(&item_id))
            .set((
                moderation_items::claimed_by.eq(Some(staff.id().to_string())),
                moderation_items::claimed_until.eq(Some(until)),
            ))
            .execute(conn)?;
        let fresh = moderation::item(conn, &item_id)?;
        Ok(enrich(conn, &staff, vec![fresh])?.remove(0))
    })
    .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

macro_rules! decision_route {
    ($name:ident, $path:literal, $doc:literal, $action:path) => {
        #[utoipa::path(
                            post,
                            path = $path,
                            tag = "admin",
                            params(("item_id" = String, Path)),
                            request_body = DecisionRequest,
                            responses(
                                (status = 200, description = $doc, body = QueueItem),
                                (status = 4XX, description = "Refused", body = ErrorEnvelope),
                            )
                        )]
        pub async fn $name(
            State(state): State<AppState>,
            staff: StaffUser,
            Path(item_id): Path<String>,
            Json(body): Json<DecisionRequest>,
        ) -> ApiResult<Json<QueueItem>> {
            staff.require(Permission::ActOnContent)?;
            let reason = reason_of(&body.reason)?;
            let out = blocking(&state.pool, move |conn| {
                conn.transaction::<_, ApiError, _>(|conn| {
                    let item = $action(conn, &item_id, staff.id(), &reason)?;
                    Ok(enrich(conn, &staff, vec![item])?.remove(0))
                })
            })
            .await?;
            Ok(Json(out))
        }
    };
}

decision_route!(
    dismiss,
    "/api/admin/queue/{item_id}/dismiss",
    "Dismissed",
    moderation::dismiss
);
decision_route!(
    remove,
    "/api/admin/queue/{item_id}/remove",
    "Removed; its author is told",
    moderation::remove
);
decision_route!(
    restore,
    "/api/admin/queue/{item_id}/restore",
    "Restored",
    moderation::restore
);
decision_route!(
    hold,
    "/api/admin/queue/{item_id}/hold",
    "Held for review",
    moderation::hold
);
decision_route!(
    release,
    "/api/admin/queue/{item_id}/release",
    "Released",
    moderation::release
);

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BulkDismiss {
    pub item_ids: Vec<String>,
    /// The one source they all share: bulk dismissal is for a flood from the
    /// same place, not for sweeping the queue.
    pub source: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/queue/dismiss",
    tag = "admin",
    request_body = BulkDismiss,
    responses(
        (status = 200, description = "How many were dismissed", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn bulk_dismiss(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<BulkDismiss>,
) -> ApiResult<Json<Value>> {
    staff.require(Permission::ActOnContent)?;
    let reason = reason_of(&body.reason)?;
    if body.item_ids.is_empty() || body.item_ids.len() > 200 {
        return Err(ApiError::BadRequest(
            "choose between 1 and 200 items".into(),
        ));
    }
    let n = blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            let mut n = 0;
            for id in &body.item_ids {
                let item = moderation::item(conn, id)?;
                let sources: Vec<String> = serde_json::from_str(&item.sources).unwrap_or_default();
                if sources != [body.source.clone()] {
                    return Err(ApiError::BadRequest(
                        "bulk dismiss is only for items flagged by this source and no other".into(),
                    ));
                }
                moderation::dismiss(conn, id, staff.id(), &reason)?;
                n += 1;
            }
            Ok(n)
        })
    })
    .await?;
    Ok(Json(json!({ "dismissed": n })))
}

// --- the word filter ------------------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WordList {
    pub terms: Vec<WordFilterTerm>,
    pub allow: Vec<String>,
    pub surfaces: Vec<String>,
}

fn word_list(conn: &mut Conn) -> ApiResult<WordList> {
    Ok(WordList {
        terms: word_filter_terms::table
            .order(word_filter_terms::term.asc())
            .select(WordFilterTerm::as_select())
            .load(conn)?,
        allow: word_filter_allow::table
            .order(word_filter_allow::word.asc())
            .select(word_filter_allow::word)
            .load(conn)?,
        surfaces: Surface::ALL
            .iter()
            .map(|s| s.as_str().to_string())
            .collect(),
    })
}

#[utoipa::path(
    get,
    path = "/api/admin/words",
    tag = "admin",
    responses(
        (status = 200, description = "The list", body = WordList),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn words(State(state): State<AppState>, staff: StaffUser) -> ApiResult<Json<WordList>> {
    staff.require(Permission::ManageWordFilter)?;
    Ok(Json(blocking(&state.pool, word_list).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TermRequest {
    pub term: String,
    /// `word` (default) or `substring`.
    #[serde(default)]
    pub match_kind: Option<String>,
    /// `block`, `hold`, `flag` or `mask`.
    pub action: String,
    /// Where it applies. Never private writing.
    pub applies_to: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TermPatch {
    #[serde(default)]
    pub term: Option<String>,
    #[serde(default)]
    pub match_kind: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub applies_to: Option<Vec<String>>,
    #[serde(default)]
    pub note: Option<String>,
}

fn check_term(term: &str, kind: &str, action: &str, surfaces: &[String]) -> ApiResult<()> {
    if filter::normalize_term(term).is_empty() {
        return Err(ApiError::BadRequest(
            "a term needs at least one letter or digit".into(),
        ));
    }
    if term.chars().count() > 100 {
        return Err(ApiError::BadRequest(
            "a term is at most 100 characters".into(),
        ));
    }
    if !["word", "substring"].contains(&kind) {
        return Err(ApiError::BadRequest(
            "match must be word or substring".into(),
        ));
    }
    if FilterAction::parse(action).is_none() {
        return Err(ApiError::BadRequest(
            "action must be block, hold, flag or mask".into(),
        ));
    }
    if surfaces.is_empty() || surfaces.iter().any(|s| Surface::parse(s).is_none()) {
        return Err(ApiError::BadRequest(format!(
            "appliesTo must be some of {}",
            Surface::ALL
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    Ok(())
}

fn changed(state: &AppState, conn: &mut Conn, staff: &StaffUser, detail: &str) -> ApiResult<()> {
    state.moderation.invalidate();
    record_staff(
        conn,
        staff.id(),
        None,
        AuditAction::WordFilterChanged,
        detail,
        None,
    )
}

#[utoipa::path(
    post,
    path = "/api/admin/words",
    tag = "admin",
    request_body = TermRequest,
    responses(
        (status = 200, description = "Added", body = WordFilterTerm),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn add_term(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<TermRequest>,
) -> ApiResult<Json<WordFilterTerm>> {
    staff.require(Permission::ManageWordFilter)?;
    let kind = body.match_kind.clone().unwrap_or_else(|| "word".into());
    check_term(&body.term, &kind, &body.action, &body.applies_to)?;
    let st = state.clone();
    let row = blocking(&state.pool, move |conn| {
        let now = clock::now_iso();
        let id = new_id();
        diesel::insert_into(word_filter_terms::table)
            .values((
                word_filter_terms::id.eq(&id),
                word_filter_terms::term.eq(body.term.trim()),
                word_filter_terms::normalized.eq(filter::normalize_term(&body.term)),
                word_filter_terms::match_kind.eq(&kind),
                word_filter_terms::action.eq(&body.action),
                word_filter_terms::applies_to.eq(json!(body.applies_to).to_string()),
                word_filter_terms::note.eq(body.note.clone().unwrap_or_default()),
                word_filter_terms::created_by.eq(staff.id()),
                word_filter_terms::created_at.eq(&now),
                word_filter_terms::updated_at.eq(&now),
            ))
            .execute(conn)?;
        changed(&st, conn, &staff, &format!("added a {} term", body.action))?;
        Ok(word_filter_terms::table
            .find(&id)
            .select(WordFilterTerm::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(row))
}

#[utoipa::path(
    patch,
    path = "/api/admin/words/{term_id}",
    tag = "admin",
    params(("term_id" = String, Path)),
    request_body = TermPatch,
    responses(
        (status = 200, description = "Changed", body = WordFilterTerm),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn update_term(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(term_id): Path<String>,
    Json(body): Json<TermPatch>,
) -> ApiResult<Json<WordFilterTerm>> {
    staff.require(Permission::ManageWordFilter)?;
    let st = state.clone();
    let row = blocking(&state.pool, move |conn| {
        let current: WordFilterTerm = word_filter_terms::table
            .find(&term_id)
            .select(WordFilterTerm::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("no such term".into()))?;
        let term = body.term.clone().unwrap_or(current.term.clone());
        let kind = body
            .match_kind
            .clone()
            .unwrap_or(current.match_kind.clone());
        let action = body.action.clone().unwrap_or(current.action.clone());
        let surfaces = body
            .applies_to
            .clone()
            .unwrap_or_else(|| serde_json::from_str(&current.applies_to).unwrap_or_default());
        check_term(&term, &kind, &action, &surfaces)?;
        diesel::update(word_filter_terms::table.find(&term_id))
            .set((
                word_filter_terms::term.eq(term.trim()),
                word_filter_terms::normalized.eq(filter::normalize_term(&term)),
                word_filter_terms::match_kind.eq(&kind),
                word_filter_terms::action.eq(&action),
                word_filter_terms::applies_to.eq(json!(surfaces).to_string()),
                word_filter_terms::note.eq(body.note.clone().unwrap_or(current.note.clone())),
                word_filter_terms::updated_at.eq(clock::now_iso()),
            ))
            .execute(conn)?;
        changed(&st, conn, &staff, "changed a term")?;
        Ok(word_filter_terms::table
            .find(&term_id)
            .select(WordFilterTerm::as_select())
            .first(conn)?)
    })
    .await?;
    Ok(Json(row))
}

#[utoipa::path(
    delete,
    path = "/api/admin/words/{term_id}",
    tag = "admin",
    params(("term_id" = String, Path)),
    responses(
        (status = 200, description = "Removed", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn delete_term(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(term_id): Path<String>,
) -> ApiResult<Json<Done>> {
    staff.require(Permission::ManageWordFilter)?;
    let st = state.clone();
    blocking(&state.pool, move |conn| {
        let n = diesel::delete(word_filter_terms::table.find(&term_id)).execute(conn)?;
        if n == 0 {
            return Err(ApiError::NotFound("no such term".into()));
        }
        changed(&st, conn, &staff, "removed a term")
    })
    .await?;
    Ok(Json(Done { ok: true }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AllowRequest {
    pub word: String,
}

#[utoipa::path(
    post,
    path = "/api/admin/words/allow",
    tag = "admin",
    request_body = AllowRequest,
    responses(
        (status = 200, description = "Allowed", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn add_allowed(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<AllowRequest>,
) -> ApiResult<Json<Done>> {
    staff.require(Permission::ManageWordFilter)?;
    let word = body.word.trim().to_lowercase();
    if word.is_empty() || word.contains(char::is_whitespace) || word.chars().count() > 60 {
        return Err(ApiError::BadRequest("allow one word at a time".into()));
    }
    let st = state.clone();
    blocking(&state.pool, move |conn| {
        diesel::insert_or_ignore_into(word_filter_allow::table)
            .values((
                word_filter_allow::word.eq(&word),
                word_filter_allow::created_by.eq(staff.id()),
                word_filter_allow::created_at.eq(clock::now_iso()),
            ))
            .execute(conn)?;
        changed(&st, conn, &staff, "allowed a word")
    })
    .await?;
    Ok(Json(Done { ok: true }))
}

#[utoipa::path(
    delete,
    path = "/api/admin/words/allow/{word}",
    tag = "admin",
    params(("word" = String, Path)),
    responses(
        (status = 200, description = "No longer allowed", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn remove_allowed(
    State(state): State<AppState>,
    staff: StaffUser,
    Path(word): Path<String>,
) -> ApiResult<Json<Done>> {
    staff.require(Permission::ManageWordFilter)?;
    let st = state.clone();
    blocking(&state.pool, move |conn| {
        diesel::delete(word_filter_allow::table.find(word.to_lowercase())).execute(conn)?;
        changed(&st, conn, &staff, "removed an allowed word")
    })
    .await?;
    Ok(Json(Done { ok: true }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestRequest {
    pub text: String,
    /// A surface, to see what would happen there. Default `comment`.
    #[serde(default)]
    pub surface: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    /// What would happen: `block`, `hold`, `flag`, `mask`, or none.
    pub action: Option<String>,
    pub matches: Vec<TestMatch>,
    pub masked: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestMatch {
    pub term: String,
    pub action: String,
    pub note: String,
}

#[utoipa::path(
    post,
    path = "/api/admin/words/test",
    tag = "admin",
    request_body = TestRequest,
    responses(
        (status = 200, description = "What would match", body = TestResult),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn test_words(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<TestRequest>,
) -> ApiResult<Json<TestResult>> {
    staff.require(Permission::ManageWordFilter)?;
    let surface = Surface::parse(body.surface.as_deref().unwrap_or("comment"))
        .ok_or_else(|| ApiError::BadRequest("unknown surface".into()))?;
    let cache = state.moderation.clone();
    let out = blocking(&state.pool, move |conn| {
        let compiled = cache.get(conn)?;
        let verdict = compiled.check(surface, &body.text);
        let terms: HashMap<String, WordFilterTerm> = word_filter_terms::table
            .select(WordFilterTerm::as_select())
            .load::<WordFilterTerm>(conn)?
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect();
        Ok(TestResult {
            action: verdict.action.map(|a| a.as_str().to_string()),
            matches: verdict
                .hits
                .iter()
                .filter_map(|h| terms.get(&h.term_id).map(|t| (t, h)))
                .map(|(t, h)| TestMatch {
                    term: t.term.clone(),
                    action: h.action.as_str().to_string(),
                    note: t.note.clone(),
                })
                .collect(),
            masked: verdict.masked,
        })
    })
    .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    /// `term,match,action,applies_to,note` — `applies_to` separated by `;`.
    pub csv: String,
}

fn csv_cell(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn csv_row(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => cells.push(std::mem::take(&mut cell)),
            (c, _) => cell.push(c),
        }
    }
    cells.push(cell);
    cells
}

#[utoipa::path(
    post,
    path = "/api/admin/words/import",
    tag = "admin",
    request_body = ImportRequest,
    responses(
        (status = 200, description = "How many were added", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn import_words(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<ImportRequest>,
) -> ApiResult<Json<Value>> {
    staff.require(Permission::ManageWordFilter)?;
    let st = state.clone();
    let added = blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            let mut added = 0;
            for (n, line) in body.csv.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with("term,") {
                    continue;
                }
                let cells = csv_row(line);
                let [term, kind, action, applies, note] = [0, 1, 2, 3, 4].map(|i| {
                    cells
                        .get(i)
                        .map(|c| c.trim().to_string())
                        .unwrap_or_default()
                });
                let kind = if kind.is_empty() {
                    "word".to_string()
                } else {
                    kind
                };
                let surfaces: Vec<String> = applies
                    .split(';')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                check_term(&term, &kind, &action, &surfaces)
                    .map_err(|e| ApiError::BadRequest(format!("line {}: {e}", n + 1)))?;
                let now = clock::now_iso();
                diesel::insert_into(word_filter_terms::table)
                    .values((
                        word_filter_terms::id.eq(new_id()),
                        word_filter_terms::term.eq(&term),
                        word_filter_terms::normalized.eq(filter::normalize_term(&term)),
                        word_filter_terms::match_kind.eq(&kind),
                        word_filter_terms::action.eq(&action),
                        word_filter_terms::applies_to.eq(json!(surfaces).to_string()),
                        word_filter_terms::note.eq(&note),
                        word_filter_terms::created_by.eq(staff.id()),
                        word_filter_terms::created_at.eq(&now),
                        word_filter_terms::updated_at.eq(&now),
                    ))
                    .execute(conn)?;
                added += 1;
            }
            changed(&st, conn, &staff, &format!("imported {added} term(s)"))?;
            Ok(added)
        })
    })
    .await?;
    Ok(Json(json!({ "added": added })))
}

#[utoipa::path(
    get,
    path = "/api/admin/words/export",
    tag = "admin",
    responses(
        (status = 200, description = "The list as CSV", content_type = "text/csv"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn export_words(
    State(state): State<AppState>,
    staff: StaffUser,
) -> ApiResult<impl IntoResponse> {
    staff.require(Permission::ManageWordFilter)?;
    let list = blocking(&state.pool, word_list).await?;
    let mut out = String::from("term,match,action,applies_to,note\n");
    for t in list.terms {
        let applies: Vec<String> = serde_json::from_str(&t.applies_to).unwrap_or_default();
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            csv_cell(&t.term),
            t.match_kind,
            t.action,
            csv_cell(&applies.join(";")),
            csv_cell(&t.note)
        ));
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    Ok((headers, out))
}

#[utoipa::path(
    post,
    path = "/api/admin/words/rescan",
    tag = "admin",
    responses(
        (status = 200, description = "What the last 30 days turned up", body = serde_json::Value),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn rescan(State(state): State<AppState>, staff: StaffUser) -> ApiResult<Json<Value>> {
    staff.require(Permission::ManageWordFilter)?;
    let cache = state.moderation.clone();
    let out = blocking(&state.pool, move |conn| {
        let compiled = cache.get(conn)?;
        let since = (clock::now() - chrono::Duration::days(30))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let mut checked = 0;
        let mut found = 0;
        let mut candidates: Vec<(&str, String, Surface)> = Vec::new();
        for (id,) in comments::table
            .filter(comments::created_at.ge(&since))
            .filter(comments::state.ne("deleted"))
            .filter(comments::moderation_state.eq("visible"))
            .select((comments::id,))
            .limit(5_000)
            .load::<(String,)>(conn)?
        {
            candidates.push(("comment", id, Surface::Comment));
        }
        for (id,) in prompt_critiques::table
            .filter(prompt_critiques::created_at.ge(&since))
            .filter(prompt_critiques::deleted_at.is_null())
            .filter(prompt_critiques::moderation_state.eq("visible"))
            .select((prompt_critiques::id,))
            .limit(5_000)
            .load::<(String,)>(conn)?
        {
            candidates.push(("critique", id, Surface::Critique));
        }
        for (id,) in prompt_entries::table
            .filter(prompt_entries::submitted_at.ge(&since))
            .filter(prompt_entries::state.eq("submitted"))
            .filter(prompt_entries::moderation_state.eq("visible"))
            .select((prompt_entries::id,))
            .limit(5_000)
            .load::<(String,)>(conn)?
        {
            candidates.push(("entry", id, Surface::Entry));
        }
        for (id,) in review_answers::table
            .filter(review_answers::created_at.ge(&since))
            .filter(review_answers::moderation_state.eq("visible"))
            .select((review_answers::id,))
            .limit(5_000)
            .load::<(String,)>(conn)?
        {
            candidates.push(("answer", id, Surface::Answer));
        }
        for (kind, id, surface) in candidates {
            let Some(subject) = moderation::subject(conn, kind, &id)? else {
                continue;
            };
            checked += 1;
            let verdict = compiled.check(surface, &subject.text);
            let Some(action) = verdict.action else {
                continue;
            };
            found += 1;
            // Already posted, so a block can only hold it for review, and a
            // mask can only be a flag: a past post's words aren't rewritten.
            let hold = matches!(action, FilterAction::Block | FilterAction::Hold);
            moderation::record(
                conn,
                &moderation::Screen {
                    text: subject.text.clone(),
                    hold,
                    flags: vec![(
                        "word_filter".into(),
                        if hold { 60 } else { 30 },
                        Some(json!({ "action": action.as_str(), "rescan": true })),
                    )],
                    ..Default::default()
                },
                kind,
                &id,
                &subject.author_id,
                "",
            )?;
        }
        record_staff(
            conn,
            staff.id(),
            None,
            AuditAction::WordFilterChanged,
            &format!("re-scanned the last 30 days: {checked} checked, {found} flagged"),
            None,
        )?;
        Ok(json!({ "checked": checked, "flagged": found }))
    })
    .await?;
    Ok(Json(out))
}

// --- external moderation ------------------------------------------------------------------

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalView {
    pub config: external::ExternalConfig,
    /// The secrets are never sent back; only whether they are set.
    pub api_key_set: bool,
    pub webhook_secret_set: bool,
    pub providers: Vec<String>,
    pub surfaces: Vec<String>,
    /// Checks waiting for the platform to come back.
    pub backlog: i64,
    /// Where the platform should send its webhooks.
    pub webhook_url: String,
}

fn external_view(conn: &mut Conn, public_url: &str) -> ApiResult<ExternalView> {
    let config = moderation::external_config(conn)?;
    Ok(ExternalView {
        api_key_set: config.api_key_sealed.is_some(),
        webhook_secret_set: config.webhook_secret_sealed.is_some(),
        backlog: external_checks::table
            .filter(external_checks::state.eq("pending"))
            .count()
            .get_result(conn)?,
        webhook_url: format!("{public_url}/api/webhooks/moderation/{}", config.provider),
        config,
        providers: vec!["none".into(), "stream".into(), "stub".into()],
        surfaces: Surface::ALL
            .iter()
            .map(|s| s.as_str().to_string())
            .collect(),
    })
}

#[utoipa::path(
    get,
    path = "/api/admin/external",
    tag = "admin",
    responses(
        (status = 200, description = "The platform's settings", body = ExternalView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_external(
    State(state): State<AppState>,
    staff: StaffUser,
) -> ApiResult<Json<ExternalView>> {
    staff.require(Permission::ManageExternalModeration)?;
    let public_url = state.config.public_url.clone();
    Ok(Json(
        blocking(&state.pool, move |conn| external_view(conn, &public_url)).await?,
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalUpdate {
    pub provider: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub surfaces: Vec<String>,
    #[serde(default)]
    pub remove_as: Option<String>,
    #[serde(default)]
    pub ban_scope: Option<String>,
    /// Set only when given; stored encrypted and never shown again.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub webhook_secret: Option<String>,
}

#[utoipa::path(
    put,
    path = "/api/admin/external",
    tag = "admin",
    request_body = ExternalUpdate,
    responses(
        (status = 200, description = "Saved", body = ExternalView),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn put_external(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<ExternalUpdate>,
) -> ApiResult<Json<ExternalView>> {
    staff.require(Permission::ManageExternalModeration)?;
    if !["none", "stream", "stub"].contains(&body.provider.as_str()) {
        return Err(ApiError::BadRequest(
            "provider must be none, stream or stub".into(),
        ));
    }
    if body.surfaces.iter().any(|s| Surface::parse(s).is_none()) {
        return Err(ApiError::BadRequest("unknown surface".into()));
    }
    let remove_as = body.remove_as.clone().unwrap_or_else(|| "hold".into());
    let ban_scope = body
        .ban_scope
        .clone()
        .unwrap_or_else(|| "discussion".into());
    if !["hold", "block"].contains(&remove_as.as_str())
        || !["discussion", "site"].contains(&ban_scope.as_str())
    {
        return Err(ApiError::BadRequest(
            "removeAs is hold or block; banScope is discussion or site".into(),
        ));
    }
    let secret = state.config.jwt_secret.clone();
    let public_url = state.config.public_url.clone();
    let out = blocking(&state.pool, move |conn| {
        let mut config = moderation::external_config(conn)?;
        let before = format!(
            "{} / {} surface(s) / remove as {} / bans {}",
            config.provider,
            config.surfaces.len(),
            config.remove_as,
            config.ban_scope
        );
        config.provider = body.provider.clone();
        config.base_url = body.base_url.clone().unwrap_or_default().trim().to_string();
        config.surfaces = body.surfaces.clone();
        config.remove_as = remove_as;
        config.ban_scope = ban_scope;
        let mut secrets_changed = Vec::new();
        if let Some(key) = body.api_key.as_deref().filter(|k| !k.trim().is_empty()) {
            config.api_key_sealed = Some(crate::domain::secrets::seal(&secret, key.trim())?);
            secrets_changed.push("API key");
        }
        if let Some(hook) = body
            .webhook_secret
            .as_deref()
            .filter(|k| !k.trim().is_empty())
        {
            config.webhook_secret_sealed =
                Some(crate::domain::secrets::seal(&secret, hook.trim())?);
            secrets_changed.push("webhook secret");
        }
        // The sealed secrets are skipped when a config is sent to a client, so
        // the stored form is built by hand.
        let mut stored = serde_json::to_value(&config)?;
        stored["apiKeySealed"] = json!(config.api_key_sealed);
        stored["webhookSecretSealed"] = json!(config.webhook_secret_sealed);
        moderation::put_setting(conn, "external", &stored, staff.id())?;
        record_staff(
            conn,
            staff.id(),
            None,
            AuditAction::ExternalModerationChanged,
            &format!(
                "{before} → {} / {} surface(s) / remove as {} / bans {}{}",
                config.provider,
                config.surfaces.len(),
                config.remove_as,
                config.ban_scope,
                if secrets_changed.is_empty() {
                    String::new()
                } else {
                    format!("; set the {}", secrets_changed.join(" and "))
                }
            ),
            None,
        )?;
        external_view(conn, &public_url)
    })
    .await?;
    Ok(Json(out))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalTestResult {
    pub recommended: String,
    pub labels: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/api/admin/external/test",
    tag = "admin",
    request_body = TestRequest,
    responses(
        (status = 200, description = "What the platform says", body = ExternalTestResult),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn test_external(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<TestRequest>,
) -> ApiResult<Json<ExternalTestResult>> {
    staff.require(Permission::ManageExternalModeration)?;
    let secret = state.config.jwt_secret.clone();
    let config = blocking(&state.pool, moderation::external_config).await?;
    if !config.enabled() {
        return Err(ApiError::BadRequest("no platform is set up".into()));
    }
    let provider = config.provider(&secret)?;
    let item = CheckItem {
        surface: body.surface.clone().unwrap_or_else(|| "comment".into()),
        subject_id: format!("test-{}", new_id()),
        author_ref: external::author_ref(&secret, staff.id()),
        text: body.text.clone(),
    };
    let verdict = tokio::time::timeout(std::time::Duration::from_secs(10), provider.check(&item))
        .await
        .map_err(|_| ApiError::internal("the platform did not answer in time"))??;
    Ok(Json(ExternalTestResult {
        recommended: match verdict.recommended {
            Recommended::Keep => "keep",
            Recommended::Flag => "flag",
            Recommended::Remove => "remove",
        }
        .into(),
        labels: verdict.labels,
    }))
}

#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModerationSettings {
    pub thresholds: moderation::Thresholds,
    /// `warning`, `kind:scope:days` or `kind:scope:permanent`.
    pub ladder: Vec<String>,
}

#[utoipa::path(
    get,
    path = "/api/admin/moderation-settings",
    tag = "admin",
    responses(
        (status = 200, description = "Thresholds and the escalation ladder", body = ModerationSettings),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn get_settings(
    State(state): State<AppState>,
    staff: StaffUser,
) -> ApiResult<Json<ModerationSettings>> {
    staff.require(Permission::ManageExternalModeration)?;
    let out = blocking(&state.pool, |conn| {
        Ok(ModerationSettings {
            thresholds: moderation::thresholds(conn)?,
            ladder: moderation::ladder(conn)?,
        })
    })
    .await?;
    Ok(Json(out))
}

#[utoipa::path(
    put,
    path = "/api/admin/moderation-settings",
    tag = "admin",
    request_body = ModerationSettings,
    responses(
        (status = 200, description = "Saved", body = ModerationSettings),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn put_settings(
    State(state): State<AppState>,
    staff: StaffUser,
    Json(body): Json<ModerationSettings>,
) -> ApiResult<Json<ModerationSettings>> {
    staff.require(Permission::ManageExternalModeration)?;
    let t = &body.thresholds;
    if [
        t.reporters,
        t.reporter_days,
        t.removals,
        t.removal_days,
        t.comments_first_hour,
    ]
    .iter()
    .any(|n| *n < 1 || *n > 10_000)
    {
        return Err(ApiError::BadRequest(
            "every threshold is between 1 and 10,000".into(),
        ));
    }
    if body.ladder.is_empty() || body.ladder.len() > 10 {
        return Err(ApiError::BadRequest(
            "the ladder has between 1 and 10 steps".into(),
        ));
    }
    let saved = body.ladder.clone();
    blocking(&state.pool, move |conn| {
        moderation::put_setting(
            conn,
            "thresholds",
            &serde_json::to_value(&body.thresholds)?,
            staff.id(),
        )?;
        moderation::put_setting(conn, "ladder", &json!(body.ladder), staff.id())?;
        record_staff(
            conn,
            staff.id(),
            None,
            AuditAction::ExternalModerationChanged,
            "changed the thresholds or the escalation ladder",
            None,
        )
    })
    .await?;
    let out = blocking(&state.pool, |conn| {
        Ok(ModerationSettings {
            thresholds: moderation::thresholds(conn)?,
            ladder: moderation::ladder(conn)?,
        })
    })
    .await?;
    debug_assert_eq!(out.ladder, saved);
    Ok(Json(out))
}

// --- the log ---------------------------------------------------------------------------------

/// Every action that belongs in the moderation log.
const LOGGED: &[&str] = &[
    "account_created_by_staff",
    "account_edited",
    "password_reset_sent",
    "sessions_revoked_by_staff",
    "account_deactivated",
    "account_reactivated",
    "account_deleted_by_staff",
    "moderator_granted",
    "moderator_revoked",
    "restriction_added",
    "restriction_changed",
    "restriction_lifted",
    "appeal_decided",
    "permanent_ban_listed",
    "permanent_ban_removed",
    "account_purged",
    "content_removed",
    "content_restored",
    "content_held",
    "report_dismissed",
    "word_filter_changed",
    "external_moderation_changed",
    "staff_note_added",
    "blocked_domain_changed",
    "account_content_removed",
    "work_handed_over",
    "two_factor_disabled",
    "prompt_edited",
    "admin_granted",
    "admin_revoked",
];

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub id: String,
    pub action: String,
    pub detail: String,
    pub reason: Option<String>,
    pub created_at: String,
    /// A staff member, or `external:<provider>:<moderator>`.
    pub actor: Option<AuthorCard>,
    pub subject: Option<AuthorCard>,
    /// `scheduled deletion` for a purge; `external` for the platform.
    pub origin: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogPage {
    pub lines: Vec<LogLine>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
#[serde(rename_all = "camelCase")]
pub struct LogQuery {
    #[serde(default)]
    pub actor: Option<String>,
    /// The account it was about.
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    /// ISO dates.
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub cursor: Option<String>,
}

fn log_lines(
    conn: &mut Conn,
    staff: &StaffUser,
    query: &LogQuery,
    limit: i64,
) -> ApiResult<(Vec<LogLine>, Option<String>)> {
    let mut q = audit_events::table
        .filter(audit_events::action.eq_any(LOGGED))
        .into_boxed();
    // A moderator sees their own actions, and what was done to the account
    // they are looking at.
    if !staff.can(Permission::ViewFullLog) {
        q = match query.subject.as_deref() {
            Some(subject) => q.filter(
                audit_events::actor_id
                    .eq(staff.id().to_string())
                    .or(audit_events::subject_id.eq(subject.to_string())),
            ),
            None => q.filter(audit_events::actor_id.eq(staff.id().to_string())),
        };
    }
    if let Some(actor) = query.actor.as_deref().filter(|a| !a.is_empty()) {
        q = q.filter(audit_events::actor_id.eq(actor.to_string()));
    }
    if let Some(subject) = query.subject.as_deref().filter(|a| !a.is_empty()) {
        q = q.filter(audit_events::subject_id.eq(subject.to_string()));
    }
    if let Some(action) = query.action.as_deref().filter(|a| !a.is_empty()) {
        q = q.filter(audit_events::action.eq(action.to_string()));
    }
    if let Some(from) = query.from.as_deref().filter(|a| !a.is_empty()) {
        q = q.filter(audit_events::created_at.ge(from.to_string()));
    }
    if let Some(to) = query.to.as_deref().filter(|a| !a.is_empty()) {
        q = q.filter(audit_events::created_at.le(format!("{to}\u{10ffff}")));
    }
    if let Some((at, id)) = query.cursor.as_deref().and_then(|c| c.split_once('|')) {
        q = q.filter(
            audit_events::created_at
                .lt(at.to_string())
                .or(audit_events::created_at
                    .eq(at.to_string())
                    .and(audit_events::id.lt(id.to_string()))),
        );
    }
    let rows: Vec<StaffAuditRow> = q
        .order((audit_events::created_at.desc(), audit_events::id.desc()))
        .limit(limit + 1)
        .select(StaffAuditRow::as_select())
        .load(conn)?;
    let more = rows.len() as i64 > limit;
    let rows: Vec<StaffAuditRow> = rows.into_iter().take(limit as usize).collect();
    let ids: Vec<String> = rows
        .iter()
        .flat_map(|r| [r.actor_id.clone(), r.subject_id.clone()])
        .flatten()
        .collect();
    let cards = support::author_cards(conn, &ids)?;
    let pii = staff.can(Permission::ViewAccountPii);
    let next = more
        .then(|| rows.last().map(|r| format!("{}|{}", r.created_at, r.id)))
        .flatten();
    let lines = rows
        .into_iter()
        .map(|r| {
            let actor = r.actor_id.as_ref().map(|a| {
                cards.get(a).cloned().unwrap_or(AuthorCard {
                    id: a.clone(),
                    display_name: if a.starts_with("external:") {
                        a.clone()
                    } else if a == "system" {
                        "Scheduled job".into()
                    } else {
                        "Former staff member".into()
                    },
                })
            });
            let origin = match (&r.actor_id, r.action.as_str()) {
                (_, "account_purged") => "scheduled deletion",
                (Some(a), _) if a.starts_with("external:") => "external platform",
                _ => "staff",
            };
            LogLine {
                subject: r.subject_id.as_ref().map(|s| {
                    cards.get(s).cloned().unwrap_or(AuthorCard {
                        id: s.clone(),
                        display_name: "Deleted account".into(),
                    })
                }),
                actor,
                origin: origin.into(),
                // Details written for staff don't carry addresses, but a
                // moderator's view keeps even the free text out of caution
                // for the lines that name accounts' own security events.
                detail: if pii || !matches!(r.action.as_str(), "two_factor_disabled") {
                    r.detail
                } else {
                    String::new()
                },
                id: r.id,
                action: r.action,
                reason: r.reason,
                created_at: r.created_at,
            }
        })
        .collect();
    Ok((lines, next))
}

#[utoipa::path(
    get,
    path = "/api/admin/log",
    tag = "admin",
    params(LogQuery),
    responses(
        (status = 200, description = "What staff did, newest first", body = LogPage),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn log(
    State(state): State<AppState>,
    staff: StaffUser,
    Query(query): Query<LogQuery>,
) -> ApiResult<Json<LogPage>> {
    staff.require(Permission::ViewQueue)?;
    let page = blocking(&state.pool, move |conn| {
        let (lines, next_cursor) = log_lines(conn, &staff, &query, PAGE)?;
        Ok(LogPage { lines, next_cursor })
    })
    .await?;
    Ok(Json(page))
}

#[utoipa::path(
    get,
    path = "/api/admin/log.csv",
    tag = "admin",
    params(LogQuery),
    responses(
        (status = 200, description = "The log as CSV", content_type = "text/csv"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn log_csv(
    State(state): State<AppState>,
    staff: StaffUser,
    Query(query): Query<LogQuery>,
) -> ApiResult<impl IntoResponse> {
    staff.require(Permission::ViewFullLog)?;
    let lines = blocking(&state.pool, move |conn| {
        let mut all = Vec::new();
        let mut q = LogQuery {
            cursor: None,
            ..query
        };
        loop {
            let (lines, next) = log_lines(conn, &staff, &q, 500)?;
            all.extend(lines);
            match next {
                Some(c) if all.len() < 50_000 => q.cursor = Some(c),
                _ => break,
            }
        }
        Ok(all)
    })
    .await?;
    let mut out = String::from("when,who,origin,action,about,detail,reason\n");
    for l in lines {
        out.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            l.created_at,
            csv_cell(&l.actor.map(|a| a.display_name).unwrap_or_default()),
            l.origin,
            l.action,
            csv_cell(&l.subject.map(|a| a.display_name).unwrap_or_default()),
            csv_cell(&l.detail),
            csv_cell(&l.reason.unwrap_or_default())
        ));
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"moderation-log.csv\""),
    );
    Ok((headers, out))
}
