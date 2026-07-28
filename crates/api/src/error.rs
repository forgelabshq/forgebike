//! API error type.
//!
//! [`ApiError`] implements [`axum::response::IntoResponse`] so that handlers
//! can use `?` with `Result<_, ApiError>` — axum will automatically convert
//! the error into the correct HTTP response.
//!
//! The conversion from [`DomainError`] ensures that internal details are
//! never leaked to callers: the error message is logged at the appropriate
//! level, and only a safe, generic message is returned in the JSON body.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use forgebike_domain::DomainError;
use serde_json::json;

// ---------------------------------------------------------------------------
// Type alias — keeps handler signatures readable.
// ---------------------------------------------------------------------------

/// Shorthand for handler return types: `ApiResult<Json<Foo>>`, etc.
pub type ApiResult<T> = Result<T, ApiError>;

// ---------------------------------------------------------------------------
// ApiError
// ---------------------------------------------------------------------------

/// An error that can be returned from any HTTP handler.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    /// Construct an error with an explicit status code and message.
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    /// 500 Internal Server Error — details logged, generic message returned.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    /// 404 Not Found.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    /// 401 Unauthorised.
    #[must_use]
    pub fn unauthorised() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "Unauthorised")
    }
}

// ---------------------------------------------------------------------------
// IntoResponse — the contract axum requires
// ---------------------------------------------------------------------------

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}

// ---------------------------------------------------------------------------
// Conversion from domain errors
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
