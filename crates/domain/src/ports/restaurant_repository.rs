//! Port trait for restaurant persistence.

use async_trait::async_trait;

use crate::{
    entities::restaurant::Restaurant,
    identifiers::{RestaurantId, TenantId},
    pagination::{ListParams, Page},
    DomainError,
};

/// Data required to insert a new restaurant row.
pub struct NewRestaurant {
    pub tenant_id: TenantId,
    pub name: String,
    pub description: Option<String>,
    pub cuisine_type: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
}

#[async_trait]
pub trait RestaurantRepository: Send + Sync {
    /// Insert a new restaurant and return the persisted entity.
    async fn create(&self, new: NewRestaurant) -> Result<Restaurant, DomainError>;

    /// Find a restaurant by primary key, scoped to the given tenant.
    async fn find_by_id(
        &self,
        tenant_id: TenantId,
        id: RestaurantId,
    ) -> Result<Option<Restaurant>, DomainError>;

    /// Return a cursor-paginated list of restaurants for a tenant.
    async fn list(
        &self,
        tenant_id: TenantId,
        params: ListParams,
    ) -> Result<Page<Restaurant>, DomainError>;

    /// Persist all mutable fields of an existing restaurant.
    ///
    /// The `id` and `tenant_id` on the entity are used as WHERE-clause keys;
    /// `created_at` is never modified.
    async fn update(&self, restaurant: &Restaurant) -> Result<Restaurant, DomainError>;

    /// Delete a restaurant by primary key, scoped to the tenant.
    ///
    /// Returns `true` if a row was deleted, `false` if it was not found.
    async fn delete(&self, tenant_id: TenantId, id: RestaurantId) -> Result<bool, DomainError>;
}
