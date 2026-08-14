//! Port trait for customer-contact persistence.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::{
    entities::customer_contact::CustomerContact,
    identifiers::{CustomerContactId, RestaurantId, TenantId},
    pagination::{Cursor, Page},
    DomainError,
};

// ---------------------------------------------------------------------------
// Command / query types
// ---------------------------------------------------------------------------

/// Data required to create a new contact.
#[derive(Debug, Clone)]
pub struct NewContact {
    pub tenant_id: TenantId,
    pub restaurant_id: RestaurantId,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

/// Fields that can be changed on an existing contact.  `None` = leave as-is.
#[derive(Debug, Clone, Default)]
pub struct UpdateContact {
    pub name: Option<String>,
    pub email: Option<Option<String>>, // Some(None) clears the value
    pub phone: Option<Option<String>>,
    pub tags: Option<Vec<String>>,
    pub notes: Option<Option<String>>,
}

/// Parameters for the paginated contact list.
#[derive(Debug, Clone)]
pub struct ContactListParams {
    pub limit: i64,
    pub cursor: Option<Cursor>,
    /// When set, only contacts with this tag are returned.
    pub tag: Option<String>,
}

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

#[async_trait]
pub trait CustomerContactRepository: Send + Sync {
    /// Insert a single contact.
    async fn create(&self, contact: NewContact) -> Result<CustomerContact, DomainError>;

    /// Fetch a contact by ID, scoped to the tenant.
    async fn find_by_id(
        &self,
        tenant_id: TenantId,
        id: CustomerContactId,
    ) -> Result<Option<CustomerContact>, DomainError>;

    /// Paginated list, optionally filtered by tag.
    async fn list(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        params: ContactListParams,
    ) -> Result<Page<CustomerContact>, DomainError>;

    /// Partial update — only non-`None` fields are written.
    async fn update(
        &self,
        tenant_id: TenantId,
        id: CustomerContactId,
        update: UpdateContact,
    ) -> Result<Option<CustomerContact>, DomainError>;

    /// Delete a contact.  Returns `true` if a row was removed.
    async fn delete(&self, tenant_id: TenantId, id: CustomerContactId)
        -> Result<bool, DomainError>;

    /// Bulk-insert contacts, skipping duplicates on `(tenant_id, restaurant_id, email)`.
    /// Returns the number of rows actually inserted.
    async fn bulk_create(&self, contacts: Vec<NewContact>) -> Result<usize, DomainError>;

    /// Return all contacts eligible for a campaign send.
    /// When `tag_filter` is `Some`, only contacts with that tag are included.
    async fn list_for_campaign(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        tag_filter: Option<&str>,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<CustomerContact>, DomainError>;
}
