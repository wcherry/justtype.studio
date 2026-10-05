pub mod jwt;
pub mod password;
pub mod totp;

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use sha2::{Digest, Sha256};

use crate::error::ApiError;
use crate::state::AppState;

/// The authenticated caller. Handlers take this as an argument; if it is
/// present, the request carried a valid access token.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: String,
    pub email: String,
    /// Whether the address has been proved. Anything handed out by address —
    /// an invitation to read, a seat on a book — waits until it has.
    pub email_verified: bool,
    /// The signed-in device this request came from.
    pub session_id: String,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::Unauthorized("missing Authorization header".into()))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError::Unauthorized("expected a Bearer token".into()))?;
        let claims = jwt::verify(&state.config.jwt_secret, token)?;
        let session_id = claims
            .sid
            .ok_or_else(|| ApiError::Unauthorized("this token predates sessions".into()))?;

        // Checked on every request rather than trusted until the token runs
        // out, so revoking a session from another device takes effect now and
        // not in half an hour. One indexed lookup; at this scale, free.
        let (lookup_session, lookup_user) = (session_id.clone(), claims.sub.clone());
        let (email, verified) = crate::db::blocking(&state.pool, move |conn| {
            use crate::db::schema::{sessions, users};
            use diesel::prelude::*;
            sessions::table
                .inner_join(users::table)
                .filter(sessions::id.eq(&lookup_session))
                .filter(sessions::user_id.eq(&lookup_user))
                .filter(sessions::revoked_at.is_null())
                .select((users::email, users::email_verified_at))
                .first::<(String, Option<String>)>(conn)
                .optional()?
                .ok_or_else(|| ApiError::Unauthorized("this session has been signed out".into()))
        })
        .await?;
        Ok(AuthUser {
            id: claims.sub,
            email,
            email_verified: verified.is_some(),
            session_id,
        })
    }
}

/// Where a request came from, for telling sessions apart in a list. Never
/// used to decide anything: a user agent says whatever the client likes.
#[derive(Debug, Clone, Default)]
pub struct ClientInfo {
    pub user_agent: Option<String>,
    pub ip: Option<String>,
}

impl<S: Send + Sync> FromRequestParts<S> for ClientInfo {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let user_agent = parts
            .headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.chars().take(300).collect());
        let ip = parts
            .extensions
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|info| info.0.ip().to_string());
        Ok(ClientInfo { user_agent, ip })
    }
}

/// Optional variant for endpoints that show more to a signed-in reader but are
/// still readable anonymously (public review feed, public publications).
#[derive(Debug, Clone)]
pub struct MaybeAuthUser(pub Option<AuthUser>);

impl FromRequestParts<AppState> for MaybeAuthUser {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(MaybeAuthUser(
            AuthUser::from_request_parts(parts, state).await.ok(),
        ))
    }
}

/// Refresh tokens are opaque random strings; only their digest is stored, so a
/// leaked database cannot be replayed against the API.
pub fn hash_refresh_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// A random opaque token for a link: a password reset, an address to prove.
pub fn generate_link_token() -> String {
    generate_refresh_token()
}

pub fn generate_refresh_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
