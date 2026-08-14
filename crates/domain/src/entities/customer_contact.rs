//! Customer contact entity.
//!
//! Contacts are restaurant patrons collected by the owner for marketing
//! purposes.  They can be added individually or bulk-imported from a JSON
//! list and tagged for audience segmentation.

use chrono::{DateTime, Utc};

use crate::identifiers::{CustomerContactId, RestaurantId, TenantId};

/// A marketing contact associated with a restaurant.
#[derive(Debug, Clone)]
pub struct CustomerContact {
    pub id: CustomerContactId,
    pub tenant_id: TenantId,
    pub restaurant_id: RestaurantId,
    /// Full display name.
    pub name: String,
    /// Email address — required for email campaigns.
    pub email: Option<String>,
    /// Phone number — required for SMS campaigns.
    pub phone: Option<String>,
    /// Free-form tags used for audience segmentation (e.g. `"vip"`, `"newsletter"`).
    pub tags: Vec<String>,
    /// Optional internal notes visible only to the restaurant owner.
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
