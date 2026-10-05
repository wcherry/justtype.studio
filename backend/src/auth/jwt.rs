use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    /// The session this token belongs to. Tokens minted before sessions
    /// existed have none, and are refused so the client refreshes into one.
    #[serde(default)]
    pub sid: Option<String>,
    pub iat: i64,
    pub exp: i64,
}

pub fn issue(
    secret: &str,
    user_id: &str,
    email: &str,
    session_id: &str,
    ttl_minutes: i64,
) -> ApiResult<String> {
    let now = chrono::Utc::now();
    let claims = Claims {
        sub: user_id.to_string(),
        email: email.to_string(),
        sid: Some(session_id.to_string()),
        iat: now.timestamp(),
        exp: (now + chrono::Duration::minutes(ttl_minutes)).timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| ApiError::internal(format!("token signing failed: {e}")))
}

pub fn verify(secret: &str, token: &str) -> ApiResult<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| ApiError::Unauthorized("invalid or expired access token".into()))
}
