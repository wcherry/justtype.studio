use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    /// A conflict the client is expected to act on rather than just report, so
    /// it carries the state needed to resolve it.
    #[error("{message}")]
    ConflictWith {
        message: String,
        details: serde_json::Value,
    },
    /// Too much, too fast. Carries the wait in seconds so a client can say
    /// something better than "try again".
    #[error("{message}")]
    TooManyRequests { message: String, retry_after: i64 },
    /// The password was right and the account wants a code from an
    /// authenticator as well. Its own code, so a client knows to ask for one
    /// rather than report a failure.
    #[error("{0}")]
    TwoFactorRequired(String),
    #[error("{0}")]
    Internal(String),
}

impl ApiError {
    pub fn internal(msg: impl Into<String>) -> Self {
        ApiError::Internal(msg.into())
    }

    fn parts(&self) -> (StatusCode, &'static str) {
        match self {
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            ApiError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
            ApiError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            ApiError::ConflictWith { .. } => (StatusCode::CONFLICT, "conflict"),
            ApiError::TooManyRequests { .. } => {
                (StatusCode::TOO_MANY_REQUESTS, "too_many_requests")
            }
            ApiError::TwoFactorRequired(_) => (StatusCode::UNAUTHORIZED, "two_factor_required"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.parts();
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(error = %self, "request failed");
        }
        let mut error = json!({ "code": code, "message": self.to_string() });
        // The id to quote when something goes wrong: the same one is on every
        // log line the request wrote.
        if let Some(id) = crate::ops::current_request_id() {
            error["requestId"] = json!(id);
        }
        if let ApiError::ConflictWith { details, .. } = &self {
            error["details"] = details.clone();
        }
        if let ApiError::TooManyRequests { retry_after, .. } = &self {
            error["details"] = json!({ "retryAfter": retry_after });
        }
        (status, Json(json!({ "error": error }))).into_response()
    }
}

impl From<diesel::result::Error> for ApiError {
    fn from(e: diesel::result::Error) -> Self {
        use diesel::result::{DatabaseErrorKind, Error};
        match e {
            Error::NotFound => ApiError::NotFound("resource not found".into()),
            Error::DatabaseError(DatabaseErrorKind::UniqueViolation, info) => {
                ApiError::Conflict(info.message().to_string())
            }
            other => ApiError::Internal(other.to_string()),
        }
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError::BadRequest(format!("invalid json: {e}"))
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        ApiError::Internal(format!("storage error: {e}"))
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// The shape every failure comes back as. Declared for the generated schema so
/// clients can type their error handling instead of guessing.
#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct ErrorEnvelope {
    pub error: ErrorBody,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    /// `bad_request`, `unauthorized`, `two_factor_required`, `forbidden`,
    /// `not_found`, `conflict`, `too_many_requests`, `internal`.
    pub code: String,
    pub message: String,
    /// Present when the client is expected to act on it — see a scene save conflict.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
    /// The request's id, as in its `x-request-id` header and its log lines.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}
