//! The decoded identity of an authenticated request.
//!
//! [`AuthIdentity`] is injected into axum request extensions by the auth
//! middleware after a valid JWT is verified, and extracted by protected
//! handlers via the [`crate`] extractor machinery.

use crate::{
    entities::user::UserRole,
    identifiers::{TenantId, UserId},
};

/// Represents the verified identity carried by every authenticated request.
#[derive(Debug, Clone)]
pub struct AuthIdentity {
    pub user_id: UserId,
    pub tenant_id: TenantId,
    pub role: UserRole,
}
