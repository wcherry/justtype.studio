pub mod jwt;
pub mod password;

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
    /// Carried from the token so log lines and audit trails do not need a
    /// database round trip.
    #[allow(dead_code)]
    pub email: String,
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
        Ok(AuthUser {
            id: claims.sub,
            email: claims.email,
        })
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

pub fn generate_refresh_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}
