//! Menu item entity.

use chrono::{DateTime, Utc};

use crate::identifiers::{MenuItemId, RestaurantId, TenantId};

/// A single dish or drink on a restaurant's menu.
///
/// `tenant_id` is denormalized from the parent [`Restaurant`](crate::entities::restaurant::Restaurant)
/// so that tenant-scoped queries need only one index scan.
#[derive(Debug, Clone)]
pub struct MenuItem {
    pub id: MenuItemId,
    pub restaurant_id: RestaurantId,
    pub tenant_id: TenantId,
    pub name: String,
    pub description: Option<String>,
    /// Price in the smallest currency unit (pence, cents, etc.).
    /// `None` indicates an unset or market price.
    pub price_cents: Option<i64>,
    pub category: Option<String>,
    pub is_available: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
