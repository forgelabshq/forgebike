//! Port trait for menu-item persistence.

use async_trait::async_trait;

use crate::{
    entities::menu_item::MenuItem,
    identifiers::{MenuItemId, RestaurantId, TenantId},
    pagination::{ListParams, Page},
    DomainError,
};

/// Data required to insert a new menu item row.
pub struct NewMenuItem {
    pub restaurant_id: RestaurantId,
    pub tenant_id: TenantId,
    pub name: String,
    pub description: Option<String>,
    pub price_cents: Option<i64>,
    pub category: Option<String>,
    pub is_available: bool,
}

#[async_trait]
pub trait MenuItemRepository: Send + Sync {
    /// Insert a new menu item and return the persisted entity.
    async fn create(&self, new: NewMenuItem) -> Result<MenuItem, DomainError>;

    /// Find a menu item by primary key, scoped to the given tenant.
    async fn find_by_id(
        &self,
        tenant_id: TenantId,
        id: MenuItemId,
    ) -> Result<Option<MenuItem>, DomainError>;

    /// Return a cursor-paginated list of menu items for a restaurant.
    async fn list(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        params: ListParams,
    ) -> Result<Page<MenuItem>, DomainError>;

    /// Persist all mutable fields of an existing menu item.
    async fn update(&self, item: &MenuItem) -> Result<MenuItem, DomainError>;

    /// Delete a menu item by primary key, scoped to the tenant.
    ///
    /// Returns `true` if a row was deleted, `false` if it was not found.
    async fn delete(&self, tenant_id: TenantId, id: MenuItemId) -> Result<bool, DomainError>;
}
