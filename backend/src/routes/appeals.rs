//! The writer's side of a restriction: seeing where they stand, appealing
//! once, and — for someone who can't sign in — appealing from an emailed link
//! and having a copy of their work emailed to them.
//!
//! Nothing here shows a writer the staff member who set a restriction or the
//! internal note on it.
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::{self, AuthUser};
use crate::clock;
use crate::db::models::*;
use crate::db::schema::*;
use crate::db::{blocking, Conn};
use crate::domain::new_id;
use crate::domain::restrictions as rules;
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::limit::Action;
use crate::state::AppState;

use super::account::Done;
use super::auth::normalize_email;

const MAX_APPEAL: usize = 2_000;

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MyAppeal {
    /// `open`, `upheld` or `overturned`.
    pub state: String,
    pub decision_text: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MyRestriction {
    pub id: String,
    pub kind: String,
    pub scope: String,
    pub starts_at: String,
    /// Absent when permanent.
    pub ends_at: Option<String>,
    pub reason_code: String,
    pub reason_text: String,
    pub in_force: bool,
    pub lifted: bool,
    /// Ends in the account being deleted.
    pub closing: bool,
    pub appeal: Option<MyAppeal>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Closing {
    /// `read_only` or `ban`.
    pub kind: String,
    pub purge_at: String,
    pub days_left: i64,
    /// Waiting on an appeal: the deletion happens only once it is decided.
    pub appeal_open: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Standing {
    /// In force now (warnings excepted).
    pub active: Vec<MyRestriction>,
    /// Everything before, newest first, warnings included.
    pub history: Vec<MyRestriction>,
    /// Present when the account is counting down to deletion.
    pub closing: Option<Closing>,
}

fn mine(conn: &mut Conn, user_id: &str) -> ApiResult<Standing> {
    let rows: Vec<Restriction> = restrictions::table
        .filter(restrictions::user_id.eq(user_id))
        .order(restrictions::created_at.desc())
        .select(Restriction::as_select())
        .load(conn)?;
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let appeal_rows: Vec<Appeal> = appeals::table
        .filter(appeals::restriction_id.eq_any(&ids))
        .select(Appeal::as_select())
        .load(conn)?;
    let now = clock::now_iso();
    let all: Vec<MyRestriction> = rows
        .iter()
        .map(|r| MyRestriction {
            id: r.id.clone(),
            kind: r.kind.clone(),
            scope: r.scope.clone(),
            starts_at: r.starts_at.clone(),
            ends_at: if r.kind == "warning" {
                None
            } else {
                r.ends_at.clone()
            },
            reason_code: r.reason_code.clone(),
            reason_text: r.reason_text.clone(),
            in_force: r.kind != "warning" && rules::in_force(r, &now),
            lifted: r.lifted_at.is_some(),
            closing: rules::is_closing(r),
            appeal: appeal_rows
                .iter()
                .find(|a| a.restriction_id == r.id)
                .map(|a| MyAppeal {
                    state: a.state.clone(),
                    decision_text: a.decision_text.clone(),
                    created_at: a.created_at.clone(),
                }),
        })
        .collect();
    let purge_at: Option<String> = users::table
        .find(user_id)
        .select(users::purge_at)
        .first(conn)?;
    let closing = purge_at.and_then(|p| {
        let r = rows
            .iter()
            .find(|r| rules::is_closing(r) && rules::in_force(r, &now))?;
        let left = chrono::DateTime::parse_from_rfc3339(&p)
            .map(|t| (t.with_timezone(&chrono::Utc) - clock::now()).num_days())
            .unwrap_or(0);
        Some(Closing {
            kind: r.kind.clone(),
            appeal_open: appeal_rows
                .iter()
                .any(|a| a.restriction_id == r.id && a.state == "open"),
            purge_at: p,
            days_left: left.max(0),
        })
    });
    let (active, history): (Vec<_>, Vec<_>) = all.into_iter().partition(|r| r.in_force);
    Ok(Standing {
        active,
        history,
        closing,
    })
}

#[utoipa::path(
    get,
    path = "/api/me/restrictions",
    tag = "account",
    responses(
        (status = 200, description = "Where the account stands", body = Standing),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn my_standing(
    State(state): State<AppState>,
    user: AuthUser,
) -> ApiResult<Json<Standing>> {
    let standing = blocking(&state.pool, move |conn| mine(conn, &user.id)).await?;
    Ok(Json(standing))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AppealRequest {
    pub body: String,
}

fn open_appeal(conn: &mut Conn, r: &Restriction, body: &str) -> ApiResult<MyAppeal> {
    let body = body.trim();
    if body.is_empty() {
        return Err(ApiError::BadRequest("say why you are appealing".into()));
    }
    if body.chars().count() > MAX_APPEAL {
        return Err(ApiError::BadRequest(format!(
            "an appeal is at most {MAX_APPEAL} characters"
        )));
    }
    if r.kind == "warning" || r.lifted_at.is_some() || !rules::in_force(r, &clock::now_iso()) {
        return Err(ApiError::Conflict(
            "there is nothing to appeal: that restriction is not in force".into(),
        ));
    }
    let now = clock::now_iso();
    // One appeal per restriction: the column is unique.
    diesel::insert_into(appeals::table)
        .values((
            appeals::id.eq(new_id()),
            appeals::restriction_id.eq(&r.id),
            appeals::body.eq(body),
            appeals::state.eq("open"),
            appeals::created_at.eq(&now),
        ))
        .execute(conn)
        .map_err(|_| ApiError::Conflict("you have already appealed this restriction".into()))?;
    Ok(MyAppeal {
        state: "open".into(),
        decision_text: None,
        created_at: now,
    })
}

#[utoipa::path(
    post,
    path = "/api/me/restrictions/{restriction_id}/appeal",
    tag = "account",
    params(("restriction_id" = String, Path)),
    request_body = AppealRequest,
    responses(
        (status = 200, description = "Filed", body = MyAppeal),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn appeal(
    State(state): State<AppState>,
    user: AuthUser,
    Path(restriction_id): Path<String>,
    Json(body): Json<AppealRequest>,
) -> ApiResult<Json<MyAppeal>> {
    let out = blocking(&state.pool, move |conn| {
        let r: Restriction = restrictions::table
            .find(&restriction_id)
            .filter(restrictions::user_id.eq(&user.id))
            .select(Restriction::as_select())
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("no such restriction".into()))?;
        open_appeal(conn, &r, &body.body)
    })
    .await?;
    Ok(Json(out))
}

// --- a banned writer, who can't sign in ---------------------------------------

/// The restriction an emailed appeal token is for, without spending it.
fn peek(conn: &mut Conn, token: &str) -> ApiResult<(EmailToken, Restriction)> {
    let row: EmailToken = email_tokens::table
        .filter(email_tokens::token_hash.eq(auth::hash_refresh_token(token.trim())))
        .filter(email_tokens::purpose.like("appeal:%"))
        .select(EmailToken::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::BadRequest("that link is not one we sent".into()))?;
    if row.used_at.is_some() {
        return Err(ApiError::BadRequest(
            "that link has already been used".into(),
        ));
    }
    if row.expires_at.as_str() < clock::now_iso().as_str() {
        return Err(ApiError::BadRequest("that link has expired".into()));
    }
    let id = row.purpose.trim_start_matches("appeal:").to_string();
    let r: Restriction = restrictions::table
        .find(&id)
        .select(Restriction::as_select())
        .first(conn)
        .optional()?
        .ok_or_else(|| ApiError::NotFound("that restriction no longer exists".into()))?;
    Ok((row, r))
}

#[utoipa::path(
    get,
    path = "/api/ban/appeal/{token}",
    tag = "account",
    params(("token" = String, Path)),
    responses(
        (status = 200, description = "What the link is for", body = MyRestriction),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn appeal_link(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> ApiResult<Json<MyRestriction>> {
    let out = blocking(&state.pool, move |conn| {
        let (_, r) = peek(conn, &token)?;
        let standing = mine(conn, &r.user_id)?;
        standing
            .active
            .into_iter()
            .chain(standing.history)
            .find(|m| m.id == r.id)
            .ok_or_else(|| ApiError::NotFound("that restriction no longer exists".into()))
    })
    .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkAppealRequest {
    pub token: String,
    pub body: String,
}

#[utoipa::path(
    post,
    path = "/api/ban/appeal",
    tag = "account",
    request_body = LinkAppealRequest,
    responses(
        (status = 200, description = "Filed", body = MyAppeal),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn appeal_from_link(
    State(state): State<AppState>,
    Json(body): Json<LinkAppealRequest>,
) -> ApiResult<Json<MyAppeal>> {
    let out = blocking(&state.pool, move |conn| {
        conn.transaction::<_, ApiError, _>(|conn| {
            let (row, r) = peek(conn, &body.token)?;
            let filed = open_appeal(conn, &r, &body.body)?;
            // Single use.
            diesel::update(email_tokens::table.find(&row.id))
                .set(email_tokens::used_at.eq(Some(clock::now_iso())))
                .execute(conn)?;
            Ok(filed)
        })
    })
    .await?;
    Ok(Json(out))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct BanExportRequest {
    pub email: String,
}

/// Asks for a copy of the work of a permanently banned account to be emailed
/// to the account's address, until the account is purged. The answer is the
/// same whatever the address is, so this can't be used to find out who is
/// banned.
#[utoipa::path(
    post,
    path = "/api/ban/export",
    tag = "account",
    request_body = BanExportRequest,
    responses(
        (status = 200, description = "Sent if there is such an account", body = Done),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn ban_export(
    State(state): State<AppState>,
    Json(body): Json<BanExportRequest>,
) -> ApiResult<Json<Done>> {
    let email = normalize_email(&body.email)?;
    state.limiter.check(&email, Action::Mail, 3)?;
    let found = blocking(&state.pool, {
        let email = email.clone();
        move |conn| {
            let user: Option<User> = users::table
                .filter(users::email.eq(&email))
                .select(User::as_select())
                .first(conn)
                .optional()?;
            let Some(user) = user else { return Ok(None) };
            let now = clock::now_iso();
            let active = rules::active(conn, &user.id)?;
            let banned = rules::site_ban(&active).is_some_and(|b| b.ends_at.is_none());
            let purge: Option<String> = users::table
                .find(&user.id)
                .select(users::purge_at)
                .first(conn)?;
            // Only until the account is purged.
            let live = purge.is_none_or(|p| p > now);
            Ok((banned && live).then_some(user))
        }
    })
    .await?;
    if let Some(user) = found {
        if let Err(e) =
            super::privacy::email_export(&state, &user.id, &user.email, &user.display_name, 7).await
        {
            tracing::error!(error = %e, "could not build a banned writer's export");
        }
    }
    Ok(Json(Done { ok: true }))
}

#[utoipa::path(
    get,
    path = "/api/ban/export/{token}",
    tag = "account",
    params(("token" = String, Path)),
    responses(
        (status = 200, description = "The zip", content_type = "application/zip"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn download_export(
    State(state): State<AppState>,
    Path(token): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let digest = auth::hash_refresh_token(token.trim());
    let path = blocking(&state.pool, move |conn| {
        let row: (String, String, Option<String>) = emailed_exports::table
            .filter(emailed_exports::token_hash.eq(&digest))
            .select((
                emailed_exports::path,
                emailed_exports::expires_at,
                emailed_exports::downloaded_at,
            ))
            .first(conn)
            .optional()?
            .ok_or_else(|| ApiError::NotFound("that link is not one we sent".into()))?;
        if row.1.as_str() < clock::now_iso().as_str() {
            return Err(ApiError::NotFound("that link has expired".into()));
        }
        diesel::update(emailed_exports::table.filter(emailed_exports::token_hash.eq(&digest)))
            .set(emailed_exports::downloaded_at.eq(Some(clock::now_iso())))
            .execute(conn)?;
        Ok(row.0)
    })
    .await?;
    let bytes = std::fs::read(&path)
        .map_err(|_| ApiError::NotFound("that copy is no longer kept".into()))?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"justtype-export.zip\""),
    );
    Ok((StatusCode::OK, headers, bytes))
}
