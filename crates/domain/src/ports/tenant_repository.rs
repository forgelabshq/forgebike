//! Port trait for tenant persistence.

use async_trait::async_trait;

use crate::{entities::tenant::Tenant, error::DomainError, identifiers::TenantId};

/// Data required to persist a new tenant row.
pub struct NewTenant {
    pub name: String,
}

#[async_trait]
pub trait TenantRepository: Send + Sync {
    /// Insert a new tenant and return the persisted entity.
    async fn create(&self, new_tenant: NewTenant) -> Result<Tenant, DomainError>;

    /// Look up a tenant by its primary key.
    async fn find_by_id(&self, id: TenantId) -> Result<Option<Tenant>, DomainError>;
}
