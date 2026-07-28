//! Domain-level error type.
//!
//! Every application service and repository returns `Result<T, DomainError>`.
//! The API layer then maps each variant to the appropriate HTTP status code.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    /// A requested resource could not be found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// The caller is not authenticated.
    #[error("Unauthorised")]
    Unauthorised,

    /// The caller is authenticated but lacks the required permission.
    #[error("Forbidden")]
    Forbidden,

    /// Input failed a business-rule or format check.
    #[error("Validation error: {0}")]
    Validation(String),

    /// The operation would violate a uniqueness or state constraint.
    #[error("Conflict: {0}")]
    Conflict(String),

    /// A call to an external service (Google, Yelp, `OpenAI`, …) failed.
    #[error("External service error: {0}")]
    ExternalService(String),

    /// An unexpected internal error. Details are logged but never exposed to
    /// the caller.
    #[error("Internal error: {0}")]
    Internal(String),
}
