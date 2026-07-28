//! Port trait for user persistence.

use async_trait::async_trait;

use crate::{
    entities::user::{User, UserRole},
    error::DomainError,
    identifiers::{TenantId, UserId},
};

/// Data required to persist a new user row.
pub struct NewUser {
    pub tenant_id: TenantId,
    pub email: String,
    pub password_hash: String,
    pub role: UserRole,
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Insert a new user and return the persisted entity.
    async fn create(&self, new_user: NewUser) -> Result<User, DomainError>;

    /// Look up a user by their email address across all tenants.
    /// Returns `None` if no match is found.
    async fn find_by_email(&self, email: &str) -> Result<Option<User>, DomainError>;

    /// Look up a user by their primary key.
    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, DomainError>;
}
