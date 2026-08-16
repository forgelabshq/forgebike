//! Port trait for tenant persistence.

use async_trait::async_trait;

use crate::{
    entities::tenant::{PlanTier, Tenant},
    error::DomainError,
    identifiers::TenantId,
};

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

    /// Update the plan tier and (optionally) the Stripe customer ID.
    async fn update_plan(
        &self,
        id: TenantId,
        plan: PlanTier,
        stripe_customer_id: Option<&str>,
    ) -> Result<Tenant, DomainError>;

    /// Find a tenant by its Stripe customer ID.
    async fn find_by_stripe_customer_id(
        &self,
        stripe_customer_id: &str,
    ) -> Result<Option<Tenant>, DomainError>;

    /// Return all tenants (used by the billing audit background task).
    async fn list_all(&self) -> Result<Vec<Tenant>, DomainError>;
}
