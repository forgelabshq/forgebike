//! Role-based guard extractors.
//!
//! These extractors build on axum's [`Extension`] mechanism.  The auth
//! middleware inserts an [`AuthIdentity`] into the request extensions after
//! validating the JWT; these extractors then pull it out and enforce the
//! minimum required role.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Owner-only endpoint
//! async fn billing(RequireOwner(id): RequireOwner) -> impl IntoResponse { … }
//!
//! // Manager-or-above endpoint
//! async fn reports(RequireManager(id): RequireManager) -> impl IntoResponse { … }
//!
//! // Just need the identity, no role check
//! async fn me(Extension(id): Extension<AuthIdentity>) -> impl IntoResponse { … }
//! ```

use async_trait::async_trait;
use axum::{extract::FromRequestParts, http::request::Parts, Extension};

use forgebike_domain::entities::{auth_identity::AuthIdentity, user::UserRole};

use crate::error::ApiError;

// ---------------------------------------------------------------------------
// RequireOwner — only the `owner` role is admitted.
// ---------------------------------------------------------------------------

pub struct RequireOwner(pub AuthIdentity);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for RequireOwner {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Extension(identity) = Extension::<AuthIdentity>::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::unauthorised())?;

        if identity.role != UserRole::Owner {
            return Err(ApiError::new(
                axum::http::StatusCode::FORBIDDEN,
                "Owner access required",
            ));
        }
        Ok(Self(identity))
    }
}

// ---------------------------------------------------------------------------
// RequireManager — `owner` or `manager` roles are admitted.
// ---------------------------------------------------------------------------

pub struct RequireManager(pub AuthIdentity);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for RequireManager {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Extension(identity) = Extension::<AuthIdentity>::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::unauthorised())?;

        if identity.role == UserRole::Viewer {
            return Err(ApiError::new(
                axum::http::StatusCode::FORBIDDEN,
                "Manager or owner access required",
            ));
        }
        Ok(Self(identity))
    }
}
