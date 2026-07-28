//! Port trait for refresh-token storage (Redis-backed in production).
//!
//! The port accepts **raw** token strings; the concrete adapter is responsible
//! for hashing them (SHA-256) before writing to the store.  This keeps
//! cryptographic primitives out of the domain layer.

use async_trait::async_trait;

use crate::{
    entities::user::UserRole,
    error::DomainError,
    identifiers::{TenantId, UserId},
};

/// The data associated with a stored refresh token.
#[derive(Debug, Clone)]
pub struct StoredTokenData {
    pub user_id: UserId,
    pub tenant_id: TenantId,
    pub role: UserRole,
}

#[async_trait]
pub trait TokenStore: Send + Sync {
    /// Persist a refresh token entry with the given TTL (seconds).
    /// The adapter hashes `raw_token` before writing.
    async fn store(
        &self,
        raw_token: &str,
        data: StoredTokenData,
        ttl_secs: u64,
    ) -> Result<(), DomainError>;

    /// Retrieve the data associated with a raw refresh token.
    /// Returns `None` if the token does not exist or has expired.
    async fn get(&self, raw_token: &str) -> Result<Option<StoredTokenData>, DomainError>;

    /// Remove a refresh token — called on logout and token rotation.
    async fn revoke(&self, raw_token: &str) -> Result<(), DomainError>;
}
