//! Auth-specific error type for the application layer.

use thiserror::Error;

use forgebike_domain::DomainError;

#[derive(Debug, Error)]
pub enum AuthError {
    /// The email/password combination did not match any user.
    /// Deliberately vague — do not indicate *which* field was wrong.
    #[error("Invalid email or password")]
    InvalidCredentials,

    /// A refresh token was presented that does not exist or has expired.
    #[error("Invalid or expired refresh token")]
    InvalidRefreshToken,

    /// Bubbled-up infrastructure error (DB, Redis, etc.).
    #[error(transparent)]
    Domain(#[from] DomainError),
}
