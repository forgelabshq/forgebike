//! API error type — maps domain and application errors to HTTP responses.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use forgebike_application::auth::error::AuthError;
use forgebike_domain::DomainError;
use serde_json::json;

// ---------------------------------------------------------------------------
// Type alias
// ---------------------------------------------------------------------------

pub type ApiResult<T> = Result<T, ApiError>;

// ---------------------------------------------------------------------------
// ApiError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    #[must_use]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    #[must_use]
    pub fn unauthorised() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Unauthorised")
    }

    #[must_use]
    pub fn unprocessable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

// ---------------------------------------------------------------------------
// Conversions
// ---------------------------------------------------------------------------

impl From<DomainError> for ApiError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::NotFound(msg) => Self::new(StatusCode::NOT_FOUND, msg),
            DomainError::Unauthorised => Self::new(StatusCode::UNAUTHORIZED, "Unauthorised"),
            DomainError::Forbidden => Self::new(StatusCode::FORBIDDEN, "Forbidden"),
            DomainError::Validation(msg) => Self::new(StatusCode::UNPROCESSABLE_ENTITY, msg),
            DomainError::Conflict(msg) => Self::new(StatusCode::CONFLICT, msg),
            DomainError::ExternalService(msg) => {
                tracing::error!(%msg, "External service error");
                Self::new(StatusCode::BAD_GATEWAY, "External service unavailable")
            }
            DomainError::Internal(msg) => {
                tracing::error!(%msg, "Internal error");
                Self::new(StatusCode::INTERNAL_SERVER_ERROR, "Internal server error")
            }
        }
    }
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::InvalidCredentials | AuthError::InvalidRefreshToken => {
                Self::new(StatusCode::UNAUTHORIZED, err.to_string())
            }
            AuthError::Domain(domain_err) => Self::from(domain_err),
        }
    }
}
