//! Decisions made on the external moderation platform, coming back.
//!
//! The webhook only checks the signature and stores the event; the
//! `apply_external_events` job applies them. Our queue is the source of truth:
//! each event goes through the same code as the matching staff action, with
//! the actor recorded as `external:<provider>:<their moderator>`.
//!
//! A permanent ban, or anything that would close an account, is never applied
//! automatically — closing an account deletes it. It becomes a queue item for
//! an administrator to decide.
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use diesel::prelude::*;
use serde_json::{json, Value};

use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::enums::AuditAction;
use crate::domain::external_moderation as external;
use crate::domain::moderation::{self, Flag};
use crate::domain::new_id;
use crate::domain::notices;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::account::Done;
use super::audit::record_staff;

/// The platform's signature, as hex, over the raw body.
const SIGNATURE: &str = "x-signature";

#[utoipa::path(
    post,
    path = "/api/webhooks/moderation/{provider}",
    tag = "moderation",
    params(("provider" = String, Path)),
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Stored", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn receive(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<Done>> {
    let secret = state.config.jwt_secret.clone();
    let signature = headers
        .get(SIGNATURE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    blocking(&state.pool, move |conn| {
        let config = moderation::external_config(conn)?;
        // The same answer for a bad signature, an unknown provider and a
        // platform that isn't set up: nothing is stored, nothing is said.
        let unauthorized = || ApiError::Unauthorized("that is not a signed event".into());
        if !config.enabled() || config.provider != provider {
            return Err(unauthorized());
        }
        let Some(webhook_secret) = config.webhook_secret(&secret) else {
            return Err(unauthorized());
        };
        if !external::verify_signature(&webhook_secret, &body, &signature) {
            return Err(unauthorized());
        }
        let payload: Value = serde_json::from_slice(&body)
            .map_err(|_| ApiError::BadRequest("the body is not JSON".into()))?;
        let event_id = payload
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::BadRequest("the event has no id".into()))?
            .to_string();
        let kind = payload
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        // A repeated event id is ignored: delivered twice, applied once.
        diesel::insert_or_ignore_into(external_events::table)
            .values((
                external_events::id.eq(new_id()),
                external_events::provider.eq(&provider),
                external_events::event_id.eq(&event_id),
                external_events::kind.eq(&kind),
                external_events::payload.eq(payload.to_string()),
                external_events::received_at.eq(clock::now_iso()),
            ))
            .execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(Done { ok: true }))
}

/// Applies every stored event not yet applied. Returns how many were.
pub fn apply_pending(conn: &mut Conn, secret: &str, public_url: &str) -> ApiResult<usize> {
    let events: Vec<ExternalEvent> = external_events::table
        .filter(external_events::applied_at.is_null())
        .filter(external_events::error.is_null())
        .order(external_events::received_at.asc())
        .limit(200)
        .select(ExternalEvent::as_select())
        .load(conn)?;
    let config = moderation::external_config(conn)?;
    let mut applied = 0;
    for event in events {
        let result = conn
            .transaction::<_, ApiError, _>(|conn| apply(conn, &config, &event, secret, public_url));
        match result {
            Ok(Applied::Done) => {
                diesel::update(external_events::table.find(&event.id))
                    .set(external_events::applied_at.eq(Some(clock::now_iso())))
                    .execute(conn)?;
                applied += 1;
            }
            // Stored, not applied, and shown on the Jobs page.
            Ok(Applied::Unrecognized) => {
                diesel::update(external_events::table.find(&event.id))
                    .set(
                        external_events::error
                            .eq(Some(format!("unrecognized event kind “{}”", event.kind))),
                    )
                    .execute(conn)?;
            }
            Err(e) => {
                diesel::update(external_events::table.find(&event.id))
                    .set(external_events::error.eq(Some(e.to_string())))
                    .execute(conn)?;
            }
        }
    }
    Ok(applied)
}

enum Applied {
    Done,
    Unrecognized,
}

fn field<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = v;
    for p in path {
        cur = cur.get(*p)?;
    }
    cur.as_str()
}

/// The queue item for the subject an event is about, created if the platform
/// flagged something we hadn't.
fn item_for(
    conn: &mut Conn,
    payload: &Value,
    config: &external::ExternalConfig,
) -> ApiResult<Option<ModerationItem>> {
    let (Some(kind), Some(id)) = (
        field(payload, &["subject", "type"]),
        field(payload, &["subject", "id"]),
    ) else {
        return Ok(None);
    };
    if let Some(item) = moderation_items::table
        .filter(moderation_items::subject_type.eq(kind))
        .filter(moderation_items::subject_id.eq(id))
        .select(ModerationItem::as_select())
        .first(conn)
        .optional()?
    {
        return Ok(Some(item));
    }
    let Some(subject) = moderation::subject(conn, kind, id)? else {
        return Ok(None);
    };
    let _ = config;
    Ok(Some(moderation::flag(
        conn,
        &Flag {
            subject_type: kind,
            subject_id: id,
            author_id: &subject.author_id,
            source: "external",
            reason: None,
            severity: 40,
            labels: Some(json!({ "from": "platform review" })),
            hold: false,
            new_report: false,
        },
    )?))
}

/// The writer an event is about: through the content it names, or by
/// recomputing the opaque reference for each account.
fn author_of(conn: &mut Conn, payload: &Value, secret: &str) -> ApiResult<Option<String>> {
    if let (Some(kind), Some(id)) = (
        field(payload, &["subject", "type"]),
        field(payload, &["subject", "id"]),
    ) {
        if let Some(subject) = moderation::subject(conn, kind, id)? {
            return Ok(Some(subject.author_id));
        }
    }
    if let Some(reference) = field(payload, &["authorRef"]) {
        let ids: Vec<String> = users::table.select(users::id).load(conn)?;
        return Ok(ids
            .into_iter()
            .find(|id| external::author_ref(secret, id) == reference));
    }
    Ok(None)
}

fn apply(
    conn: &mut Conn,
    config: &external::ExternalConfig,
    event: &ExternalEvent,
    secret: &str,
    public_url: &str,
) -> ApiResult<Applied> {
    let payload: Value = serde_json::from_str(&event.payload)?;
    let moderator = field(&payload, &["moderator"]).unwrap_or("unknown");
    let actor = format!("external:{}:{moderator}", event.provider);
    let reason = "decided on the external moderation platform";
    match event.kind.as_str() {
        "content.safe" => {
            if let Some(item) = item_for(conn, &payload, config)? {
                moderation::dismiss(conn, &item.id, &actor, reason)?;
            }
            Ok(Applied::Done)
        }
        "content.removed" => {
            if let Some(item) = item_for(conn, &payload, config)? {
                moderation::remove(conn, &item.id, &actor, reason)?;
            }
            Ok(Applied::Done)
        }
        "content.restored" => {
            if let Some(item) = item_for(conn, &payload, config)? {
                moderation::restore(conn, &item.id, &actor, reason)?;
            }
            Ok(Applied::Done)
        }
        "user.banned" | "user.suspended" => {
            let days = payload.get("days").and_then(Value::as_i64);
            let user = author_of(conn, &payload, secret)?;
            let Some(user_id) = user else {
                return Err(ApiError::NotFound(
                    "no account matches that reference".into(),
                ));
            };
            match days {
                Some(d) if [3, 7, 30].contains(&d) => {
                    restrict(conn, config, &user_id, &actor, &event.kind, d, public_url)?;
                    Ok(Applied::Done)
                }
                // Permanent, or anything else that would close the account:
                // an administrator decides.
                _ => {
                    moderation::flag(
                        conn,
                        &Flag {
                            subject_type: "account",
                            subject_id: &user_id,
                            author_id: &user_id,
                            source: "external",
                            reason: None,
                            severity: 80,
                            labels: Some(json!({
                                "recommended": "permanent_ban",
                                "by": actor,
                                "note": "External moderator recommends a permanent ban",
                            })),
                            hold: false,
                            new_report: false,
                        },
                    )?;
                    Ok(Applied::Done)
                }
            }
        }
        _ => Ok(Applied::Unrecognized),
    }
}

fn restrict(
    conn: &mut Conn,
    config: &external::ExternalConfig,
    user_id: &str,
    actor: &str,
    kind: &str,
    days: i64,
    public_url: &str,
) -> ApiResult<()> {
    // Staff are never restricted by the platform.
    let (admin, moderator): (bool, bool) = users::table
        .find(user_id)
        .select((users::is_admin, users::is_moderator))
        .first(conn)?;
    if admin || moderator {
        return Err(ApiError::Forbidden("that account is staff".into()));
    }
    let now = clock::now();
    let id = new_id();
    let restriction_kind = if kind == "user.banned" {
        "ban"
    } else {
        "suspend"
    };
    let message = "A moderator reviewing reports restricted this account.";
    diesel::insert_into(restrictions::table)
        .values((
            restrictions::id.eq(&id),
            restrictions::user_id.eq(user_id),
            restrictions::kind.eq(restriction_kind),
            restrictions::scope.eq(&config.ban_scope),
            restrictions::starts_at.eq(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            restrictions::ends_at.eq(crate::domain::restrictions::ends_at(now, Some(days))),
            restrictions::reason_code.eq("external"),
            restrictions::reason_text.eq(message),
            restrictions::created_by.eq(actor),
            restrictions::created_at.eq(now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
        ))
        .execute(conn)?;
    if restriction_kind == "ban" && config.ban_scope == "site" {
        super::account::revoke_sessions(conn, user_id, None)?;
    }
    record_staff(
        conn,
        actor,
        Some(user_id),
        AuditAction::RestrictionAdded,
        &format!("{restriction_kind} ({}) for {days} days", config.ban_scope),
        Some(message),
    )?;
    let r: Restriction = restrictions::table
        .find(&id)
        .select(Restriction::as_select())
        .first(conn)?;
    let appeal_url = if restriction_kind == "ban" && config.ban_scope == "site" {
        let user = crate::routes::support::find_user(conn, user_id)?;
        let token = super::account::mint(
            conn,
            &user,
            &format!("appeal:{id}"),
            chrono::Duration::days(60),
        )?;
        format!("{public_url}/ban/appeal?token={token}")
    } else {
        format!("{public_url}/standing")
    };
    notices::restriction_set(conn, public_url, &r, &appeal_url, None)
}
