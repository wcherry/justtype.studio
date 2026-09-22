use axum::extract::State;
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::{self, AuthUser};
use crate::db::models::{NewRefreshToken, NewUser, RefreshToken, User};
use crate::db::schema::{refresh_tokens, users};
use crate::db::{blocking, Conn};
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::state::AppState;

use super::support::find_user;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    pub user: User,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

fn normalize_email(raw: &str) -> ApiResult<String> {
    let email = raw.trim().to_lowercase();
    if !email.contains('@') || email.len() < 5 {
        return Err(ApiError::BadRequest(
            "a valid email address is required".into(),
        ));
    }
    Ok(email)
}

fn check_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < 8 {
        return Err(ApiError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }
    Ok(())
}

/// Mints a fresh access/refresh pair and records the refresh digest.
fn start_session(state: &AppState, conn: &mut Conn, user: User) -> ApiResult<SessionResponse> {
    let access_token = auth::jwt::issue(
        &state.config.jwt_secret,
        &user.id,
        &user.email,
        state.config.access_ttl_minutes,
    )?;
    let refresh_token = auth::generate_refresh_token();
    let now = now_iso();
    let expires_at = (chrono::Utc::now() + chrono::Duration::days(state.config.refresh_ttl_days))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    diesel::insert_into(refresh_tokens::table)
        .values(&NewRefreshToken {
            id: new_id(),
            user_id: user.id.clone(),
            token_hash: auth::hash_refresh_token(&refresh_token),
            expires_at,
            revoked_at: None,
            created_at: now,
        })
        .execute(conn)?;

    Ok(SessionResponse {
        user,
        access_token,
        refresh_token,
        expires_in: state.config.access_ttl_minutes * 60,
    })
}

#[utoipa::path(
    post,
    path = "/api/auth/register",
    tag = "auth",
    request_body = RegisterRequest,
    responses(
        (status = 200, description = "Success", body = SessionResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> ApiResult<Json<SessionResponse>> {
    let email = normalize_email(&body.email)?;
    check_password(&body.password)?;
    let display_name = body.display_name.trim().to_string();
    if display_name.is_empty() {
        return Err(ApiError::BadRequest("display name is required".into()));
    }
    let password_hash = auth::password::hash(&body.password)?;
    let now = now_iso();
    let new_user = NewUser {
        id: new_id(),
        email: email.clone(),
        display_name,
        password_hash,
        bio: None,
        created_at: now.clone(),
        updated_at: now,
        daily_word_goal: None,
        legal_name: None,
        mailing_address: None,
        phone: None,
    };

    let inner_state = state.clone();
    let session = blocking(&state.pool, move |conn| {
        let existing: Option<String> = users::table
            .filter(users::email.eq(&new_user.email))
            .select(users::id)
            .first(conn)
            .optional()?;
        if existing.is_some() {
            return Err(ApiError::Conflict(
                "that email is already registered".into(),
            ));
        }
        diesel::insert_into(users::table)
            .values(&new_user)
            .execute(conn)?;
        let user: User = users::table
            .find(&new_user.id)
            .select(User::as_select())
            .first(conn)?;
        start_session(&inner_state, conn, user)
    })
    .await?;

    Ok(Json(session))
}

#[utoipa::path(
    post,
    path = "/api/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Success", body = SessionResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> ApiResult<Json<SessionResponse>> {
    let email = normalize_email(&body.email)?;
    let password = body.password;
    let inner_state = state.clone();

    let session = blocking(&state.pool, move |conn| {
        let user: Option<User> = users::table
            .filter(users::email.eq(&email))
            .select(User::as_select())
            .first(conn)
            .optional()?;
        // Same error either way: do not leak which emails have accounts.
        let user =
            user.ok_or_else(|| ApiError::Unauthorized("invalid email or password".into()))?;
        if !auth::password::verify(&password, &user.password_hash) {
            return Err(ApiError::Unauthorized("invalid email or password".into()));
        }
        start_session(&inner_state, conn, user)
    })
    .await?;

    Ok(Json(session))
}

#[utoipa::path(
    post,
    path = "/api/auth/refresh",
    tag = "auth",
    request_body = RefreshRequest,
    responses(
        (status = 200, description = "Success", body = SessionResponse),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn refresh(
    State(state): State<AppState>,
    Json(body): Json<RefreshRequest>,
) -> ApiResult<Json<SessionResponse>> {
    let digest = auth::hash_refresh_token(&body.refresh_token);
    let inner_state = state.clone();

    let session = blocking(&state.pool, move |conn| {
        let row: Option<RefreshToken> = refresh_tokens::table
            .filter(refresh_tokens::token_hash.eq(&digest))
            .select(RefreshToken::as_select())
            .first(conn)
            .optional()?;
        let row = row.ok_or_else(|| ApiError::Unauthorized("unknown refresh token".into()))?;
        if row.revoked_at.is_some() {
            return Err(ApiError::Unauthorized(
                "refresh token has been revoked".into(),
            ));
        }
        if row.expires_at.as_str() < now_iso().as_str() {
            return Err(ApiError::Unauthorized("refresh token has expired".into()));
        }
        // Rotation: the presented token dies as the new one is issued.
        diesel::update(refresh_tokens::table.find(&row.id))
            .set(refresh_tokens::revoked_at.eq(now_iso()))
            .execute(conn)?;
        let user: User = users::table
            .find(&row.user_id)
            .select(User::as_select())
            .first(conn)?;
        start_session(&inner_state, conn, user)
    })
    .await?;

    Ok(Json(session))
}

#[utoipa::path(
    post,
    path = "/api/auth/logout",
    tag = "auth",
    request_body = RefreshRequest,
    responses(
        (status = 200, description = "Success"),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn logout(
    State(state): State<AppState>,
    Json(body): Json<RefreshRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let digest = auth::hash_refresh_token(&body.refresh_token);
    blocking(&state.pool, move |conn| {
        diesel::update(refresh_tokens::table.filter(refresh_tokens::token_hash.eq(&digest)))
            .set(refresh_tokens::revoked_at.eq(now_iso()))
            .execute(conn)?;
        Ok(())
    })
    .await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[utoipa::path(
    get,
    path = "/api/auth/me",
    tag = "auth",
    responses(
        (status = 200, description = "Success", body = User),
        (status = 4XX, description = "Refused", body = ErrorEnvelope),
    )
)]
pub async fn me(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<User>> {
    let found = blocking(&state.pool, move |conn| find_user(conn, &user.id)).await?;
    Ok(Json(found))
}
