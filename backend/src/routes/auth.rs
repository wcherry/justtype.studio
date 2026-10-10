use axum::extract::State;
use axum::Json;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use crate::auth::ClientInfo;
use crate::auth::{self, AuthUser};
use crate::db::models::{NewRefreshToken, NewSession, NewUser, RefreshToken, User};
use crate::db::schema::{refresh_tokens, sessions, users};
use crate::db::{blocking, Conn};
use crate::domain::{new_id, now_iso};
use crate::error::{ApiError, ApiResult, ErrorEnvelope};
use crate::limit::Action;
use crate::state::AppState;

use super::support::find_user;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
    /// Agreement to the terms in force (`GET /api/terms`). Required.
    #[serde(default)]
    pub accept_terms: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    /// The six digits from an authenticator, or a recovery code, for an
    /// account with two-factor sign-in on. Asked for with a
    /// `two_factor_required` refusal when it is missing.
    #[serde(default)]
    pub code: Option<String>,
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

pub(crate) fn normalize_email(raw: &str) -> ApiResult<String> {
    let email = raw.trim().to_lowercase();
    if !email.contains('@') || email.len() < 5 {
        return Err(ApiError::BadRequest(
            "a valid email address is required".into(),
        ));
    }
    Ok(email)
}

pub(crate) fn check_password(password: &str) -> ApiResult<()> {
    if password.chars().count() < 8 {
        return Err(ApiError::BadRequest(
            "password must be at least 8 characters".into(),
        ));
    }
    Ok(())
}

/// Opens a new signed-in session for this device and mints its first tokens.
fn start_session(
    state: &AppState,
    conn: &mut Conn,
    user: User,
    client: &ClientInfo,
) -> ApiResult<SessionResponse> {
    let now = now_iso();
    let session_id = new_id();
    diesel::insert_into(sessions::table)
        .values(&NewSession {
            id: session_id.clone(),
            user_id: user.id.clone(),
            user_agent: client.user_agent.clone(),
            ip: client.ip.clone(),
            created_at: now.clone(),
            last_seen_at: now,
            revoked_at: None,
        })
        .execute(conn)?;
    issue_tokens(state, conn, user, &session_id)
}

/// Mints an access/refresh pair inside an existing session and records the
/// refresh digest.
fn issue_tokens(
    state: &AppState,
    conn: &mut Conn,
    user: User,
    session_id: &str,
) -> ApiResult<SessionResponse> {
    let access_token = auth::jwt::issue(
        &state.config.jwt_secret,
        &user.id,
        &user.email,
        session_id,
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
            session_id: Some(session_id.to_string()),
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
    client: ClientInfo,
    Json(body): Json<RegisterRequest>,
) -> ApiResult<Json<SessionResponse>> {
    let email = normalize_email(&body.email)?;
    check_password(&body.password)?;
    let display_name = body.display_name.trim().to_string();
    if display_name.is_empty() {
        return Err(ApiError::BadRequest("display name is required".into()));
    }
    if !body.accept_terms {
        return Err(ApiError::BadRequest(
            "an account needs agreement to the terms — they say your writing stays yours".into(),
        ));
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
        updated_at: now.clone(),
        daily_word_goal: None,
        legal_name: None,
        mailing_address: None,
        phone: None,
        email_verified_at: None,
        totp_secret: None,
        totp_enabled_at: None,
        totp_last_step: None,
        terms_version: Some(super::privacy::TERMS_VERSION),
        terms_accepted_at: Some(now.clone()),
        weekly_word_goal: None,
        summaries_allowed_at: None,
        assistant_allowed_at: None,
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
        super::audit::record(
            conn,
            Some(&user.id),
            None,
            crate::domain::enums::AuditAction::TermsAccepted,
            &format!("version {}, at sign-up", super::privacy::TERMS_VERSION),
        )?;
        start_session(&inner_state, conn, user, &client)
    })
    .await?;

    // The first proof-of-address mail goes out with the account. If it fails
    // the account still stands: the person can ask for another.
    if let Err(e) = super::account::send_verification(&state, &session.user).await {
        tracing::warn!(error = %e, "could not send the first verification mail");
    }
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
    client: ClientInfo,
    Json(body): Json<LoginRequest>,
) -> ApiResult<Json<SessionResponse>> {
    let email = normalize_email(&body.email)?;
    let password = body.password;
    let code = body.code;
    let inner_state = state.clone();
    // Locked accounts are refused before the password is even looked at, so
    // a locked account cannot be used to test guesses.
    state.login_guard.check(&email)?;
    let guard = state.login_guard.clone();

    let session = blocking(&state.pool, move |conn| {
        let user: Option<User> = users::table
            .filter(users::email.eq(&email))
            .select(User::as_select())
            .first(conn)
            .optional()?;
        // Same error either way: do not leak which emails have accounts. An
        // unknown address counts towards a lock too, for the same reason.
        let wrong = || {
            guard.failed(&email);
            ApiError::Unauthorized("invalid email or password".into())
        };
        let user = user.ok_or_else(wrong)?;
        if !auth::password::verify(&password, &user.password_hash) {
            return Err(wrong());
        }
        // Asked only once the password is right, so the refusal reveals
        // nothing to someone who does not already have it.
        if user.totp_enabled_at.is_some() {
            let Some(code) = code.as_deref().filter(|c| !c.trim().is_empty()) else {
                return Err(ApiError::TwoFactorRequired(
                    "enter the code from your authenticator app".into(),
                ));
            };
            inner_state.limiter.check(
                &user.id,
                Action::SecondFactor,
                inner_state.config.second_factor_rate,
            )?;
            if let Err(e) = super::account::check_second_factor(conn, &user, code) {
                guard.failed(&email);
                return Err(e);
            }
        }
        guard.succeeded(&email);
        // The password is in hand, so this is the one moment a hash made
        // under older, cheaper settings can be made again under today's.
        if auth::password::needs_rehash(&user.password_hash) {
            let fresh = auth::password::hash(&password)?;
            diesel::update(users::table.find(&user.id))
                .set(users::password_hash.eq(&fresh))
                .execute(conn)?;
            tracing::info!(user_id = %user.id, "password rehashed under the current Argon2 cost");
        }
        start_session(&inner_state, conn, user, &client)
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
    client: ClientInfo,
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
        match &row.session_id {
            Some(session_id) => {
                // A revoked session cannot be revived by a token it issued.
                let live = diesel::update(
                    sessions::table
                        .find(session_id)
                        .filter(sessions::revoked_at.is_null()),
                )
                .set(sessions::last_seen_at.eq(now_iso()))
                .execute(conn)?;
                if live == 0 {
                    return Err(ApiError::Unauthorized(
                        "this session has been signed out".into(),
                    ));
                }
                issue_tokens(&inner_state, conn, user, session_id)
            }
            // A token from before sessions existed: give it one, so the person
            // stays signed in through the upgrade.
            None => start_session(&inner_state, conn, user, &client),
        }
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
        let session: Option<Option<String>> = refresh_tokens::table
            .filter(refresh_tokens::token_hash.eq(&digest))
            .select(refresh_tokens::session_id)
            .first(conn)
            .optional()?;
        diesel::update(refresh_tokens::table.filter(refresh_tokens::token_hash.eq(&digest)))
            .set(refresh_tokens::revoked_at.eq(now_iso()))
            .execute(conn)?;
        // Signing out ends the session, not just the token in hand.
        if let Some(Some(session_id)) = session {
            diesel::update(sessions::table.find(&session_id))
                .set(sessions::revoked_at.eq(now_iso()))
                .execute(conn)?;
        }
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
