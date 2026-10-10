//! The inbox: what happened that a writer should hear about, and yesterday's
//! summary pinned at the top until read.
//!
//! Items are written by [`notify`] inside the transaction of the event that
//! caused them, and removed by [`forget_subject`] / [`forget_context`] inside
//! the transaction that deletes what they were about. What a kind of item
//! says and links to lives in `domain::inbox`, so adding one needs no change
//! here.
use axum::extract::{Path, Query, State};
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::auth::AuthUser;
use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::{inbox as kinds, new_id};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::{self, AuthorCard};

/// Tells `user_id` that something happened. Nobody is told about their own
/// action, so a writer replying to themselves creates nothing.
pub fn notify(
    conn: &mut Conn,
    user_id: &str,
    kind: &str,
    subject: (&str, &str),
    context: Option<(&str, &str)>,
    actor_id: Option<&str>,
    detail: &str,
) -> ApiResult<()> {
    if actor_id == Some(user_id) {
        return Ok(());
    }
    debug_assert!(
        kinds::spec(kind).is_some(),
        "an inbox kind with no registry entry: {kind}"
    );
    diesel::insert_into(inbox_items::table)
        .values(&NewInboxItem {
            id: new_id(),
            user_id: user_id.into(),
            kind: kind.into(),
            subject_type: subject.0.into(),
            subject_id: subject.1.into(),
            context_type: context.map(|c| c.0.to_string()),
            context_id: context.map(|c| c.1.to_string()),
            actor_id: actor_id.map(str::to_string),
            detail: Some(detail.into()),
            created_at: clock::now_iso(),
            read_at: None,
        })
        .execute(conn)?;
    Ok(())
}

/// Deletes the items about something that is gone.
pub fn forget_subject(conn: &mut Conn, subject_type: &str, subject_id: &str) -> ApiResult<()> {
    diesel::delete(
        inbox_items::table
            .filter(inbox_items::subject_type.eq(subject_type))
            .filter(inbox_items::subject_id.eq(subject_id)),
    )
    .execute(conn)?;
    Ok(())
}

/// Deletes the items whose context is gone: everything about the critiques
/// on a deleted entry.
pub fn forget_context(conn: &mut Conn, context_type: &str, context_id: &str) -> ApiResult<()> {
    diesel::delete(
        inbox_items::table
            .filter(inbox_items::context_type.eq(context_type))
            .filter(inbox_items::context_id.eq(context_id)),
    )
    .execute(conn)?;
    Ok(())
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InboxItemView {
    pub id: String,
    pub kind: String,
    pub text: String,
    /// A path in the web app.
    pub link: String,
    pub actor: Option<AuthorCard>,
    pub created_at: String,
    pub read_at: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DigestCount {
    pub kind: String,
    pub count: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DigestView {
    /// The day it sums up.
    pub day: String,
    /// "Yesterday: 4 critiques and 2 replies on your answer to '…'. Today's
    /// prompt is open."
    pub text: String,
    pub counts: Vec<DigestCount>,
    pub read_at: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct InboxPage {
    /// The newest unread summary, pinned above the items.
    pub digest: Option<DigestView>,
    pub items: Vec<InboxItemView>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct InboxQuery {
    #[serde(default)]
    pub cursor: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

fn view_digest(conn: &mut Conn, row: InboxDigest, user_id: &str) -> ApiResult<DigestView> {
    let summary: Value = serde_json::from_str(&row.summary_json).unwrap_or(Value::Null);
    let mut text = summary
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    // Not stored: whether today's prompt is open is true when it is read, not
    // when it was written.
    let open: i64 = daily_prompts::table
        .filter(daily_prompts::day.eq(clock::day_string(super::prompts::today())))
        .count()
        .get_result(conn)?;
    if open > 0 && super::prompts::is_enrolled(conn, user_id)? {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str("Today's prompt is open.");
    }
    let counts = summary
        .get("counts")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|c| DigestCount {
                    kind: c.get("kind").and_then(Value::as_str).unwrap_or("").into(),
                    count: c.get("count").and_then(Value::as_i64).unwrap_or(0),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(DigestView {
        day: row.day,
        text,
        counts,
        read_at: row.read_at,
    })
}

pub fn view_items(conn: &mut Conn, rows: Vec<InboxItem>) -> ApiResult<Vec<InboxItemView>> {
    let actors: Vec<String> = rows.iter().filter_map(|r| r.actor_id.clone()).collect();
    let cards = support::author_cards(conn, &actors)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let detail: Value = row
                .detail
                .as_deref()
                .and_then(|d| serde_json::from_str(d).ok())
                .unwrap_or(Value::Null);
            let actor = row.actor_id.as_ref().and_then(|id| cards.get(id).cloned());
            let name = actor
                .as_ref()
                .map(|a| a.display_name.clone())
                .unwrap_or_else(|| "Someone".into());
            let (text, link) = match kinds::spec(&row.kind) {
                Some(spec) => ((spec.describe)(&name, &detail), (spec.link)(&detail)),
                None => (format!("{name}: {}", row.kind), "/inbox".into()),
            };
            InboxItemView {
                id: row.id,
                kind: row.kind,
                text,
                link,
                actor,
                created_at: row.created_at,
                read_at: row.read_at,
            }
        })
        .collect())
}

#[utoipa::path(
    get,
    path = "/api/inbox",
    tag = "inbox",
    params(InboxQuery),
    responses(
        (status = 200, description = "Newest first, the unread summary pinned", body = InboxPage),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<InboxQuery>,
) -> ApiResult<Json<InboxPage>> {
    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    let page = blocking(&state.pool, move |conn| {
        let digest: Option<InboxDigest> = inbox_digests::table
            .filter(inbox_digests::user_id.eq(&user.id))
            .filter(inbox_digests::read_at.is_null())
            .order(inbox_digests::day.desc())
            .select(InboxDigest::as_select())
            .first(conn)
            .optional()?;
        let digest = digest.map(|d| view_digest(conn, d, &user.id)).transpose()?;
        let mut q = inbox_items::table
            .filter(inbox_items::user_id.eq(&user.id))
            .into_boxed();
        // The cursor is `created_at~id` of the last item shown.
        if let Some((at, id)) = query.cursor.as_deref().and_then(|c| c.split_once('~')) {
            q = q.filter(
                inbox_items::created_at
                    .lt(at.to_string())
                    .or(inbox_items::created_at
                        .eq(at.to_string())
                        .and(inbox_items::id.lt(id.to_string()))),
            );
        }
        let rows: Vec<InboxItem> = q
            .order((inbox_items::created_at.desc(), inbox_items::id.desc()))
            .limit(limit + 1)
            .select(InboxItem::as_select())
            .load(conn)?;
        let more = rows.len() as i64 > limit;
        let rows: Vec<InboxItem> = rows.into_iter().take(limit as usize).collect();
        let next_cursor = more
            .then(|| rows.last().map(|r| format!("{}~{}", r.created_at, r.id)))
            .flatten();
        Ok(InboxPage {
            digest,
            items: view_items(conn, rows)?,
            next_cursor,
        })
    })
    .await?;
    Ok(Json(page))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UnreadCount {
    /// Unread items, plus one for an unread summary.
    pub count: i64,
}

pub fn unread(conn: &mut Conn, user_id: &str) -> ApiResult<i64> {
    let items: i64 = inbox_items::table
        .filter(inbox_items::user_id.eq(user_id))
        .filter(inbox_items::read_at.is_null())
        .count()
        .get_result(conn)?;
    let digests: i64 = inbox_digests::table
        .filter(inbox_digests::user_id.eq(user_id))
        .filter(inbox_digests::read_at.is_null())
        .count()
        .get_result(conn)?;
    Ok(items + digests.min(1))
}

#[utoipa::path(
    get,
    path = "/api/inbox/unread-count",
    tag = "inbox",
    responses(
        (status = 200, description = "For the bell", body = UnreadCount),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn unread_count(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<UnreadCount>> {
    let count = blocking(&state.pool, move |conn| unread(conn, &user.id)).await?;
    Ok(Json(UnreadCount { count }))
}

#[utoipa::path(
    post,
    path = "/api/inbox/{item_id}/read",
    tag = "inbox",
    params(("item_id" = String, Path)),
    responses(
        (status = 200, description = "Read", body = UnreadCount),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn read_one(
    State(state): State<AppState>,
    user: AuthUser,
    Path(item_id): Path<String>,
) -> ApiResult<Json<UnreadCount>> {
    let count = blocking(&state.pool, move |conn| {
        let changed = diesel::update(
            inbox_items::table
                .filter(inbox_items::id.eq(&item_id))
                .filter(inbox_items::user_id.eq(&user.id)),
        )
        .set(inbox_items::read_at.eq(Some(clock::now_iso())))
        .execute(conn)?;
        if changed == 0 {
            return Err(ApiError::NotFound("that is not in your inbox".into()));
        }
        unread(conn, &user.id)
    })
    .await?;
    Ok(Json(UnreadCount { count }))
}

#[utoipa::path(
    post,
    path = "/api/inbox/digests/{day}/read",
    tag = "inbox",
    params(("day" = String, Path)),
    responses(
        (status = 200, description = "The summary is read, and no longer pinned", body = UnreadCount),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn read_digest(
    State(state): State<AppState>,
    user: AuthUser,
    Path(day): Path<String>,
) -> ApiResult<Json<UnreadCount>> {
    let count = blocking(&state.pool, move |conn| {
        diesel::update(
            inbox_digests::table
                .filter(inbox_digests::user_id.eq(&user.id))
                .filter(inbox_digests::day.le(&day))
                .filter(inbox_digests::read_at.is_null()),
        )
        .set(inbox_digests::read_at.eq(Some(clock::now_iso())))
        .execute(conn)?;
        unread(conn, &user.id)
    })
    .await?;
    Ok(Json(UnreadCount { count }))
}

#[utoipa::path(
    post,
    path = "/api/inbox/read-all",
    tag = "inbox",
    responses(
        (status = 200, description = "Everything is read, the summary too", body = UnreadCount),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn read_all(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<UnreadCount>> {
    let count = blocking(&state.pool, move |conn| {
        let now = clock::now_iso();
        diesel::update(
            inbox_items::table
                .filter(inbox_items::user_id.eq(&user.id))
                .filter(inbox_items::read_at.is_null()),
        )
        .set(inbox_items::read_at.eq(Some(&now)))
        .execute(conn)?;
        diesel::update(
            inbox_digests::table
                .filter(inbox_digests::user_id.eq(&user.id))
                .filter(inbox_digests::read_at.is_null()),
        )
        .set(inbox_digests::read_at.eq(Some(&now)))
        .execute(conn)?;
        unread(conn, &user.id)
    })
    .await?;
    Ok(Json(UnreadCount { count }))
}

/// Everything the inbox holds about a writer, for their data export.
pub fn export_of(conn: &mut Conn, user_id: &str) -> ApiResult<Value> {
    let items: Vec<InboxItem> = inbox_items::table
        .filter(inbox_items::user_id.eq(user_id))
        .order(inbox_items::created_at.asc())
        .select(InboxItem::as_select())
        .load(conn)?;
    let items = view_items(conn, items)?;
    let digests: Vec<InboxDigest> = inbox_digests::table
        .filter(inbox_digests::user_id.eq(user_id))
        .order(inbox_digests::day.asc())
        .select(InboxDigest::as_select())
        .load(conn)?;
    Ok(json!({
        "items": items,
        "summaries": digests.iter().map(|d| json!({
            "day": d.day,
            "summary": serde_json::from_str::<Value>(&d.summary_json).unwrap_or(Value::Null),
            "readAt": d.read_at,
        })).collect::<Vec<_>>(),
    }))
}
