//! Restaurant entity.

use chrono::{DateTime, Utc};

use crate::identifiers::{RestaurantId, TenantId};

/// A physical restaurant location owned by a [`Tenant`](crate::entities::tenant::Tenant).
///
/// One tenant may own many restaurants (e.g. a restaurant group with
/// multiple branches).
#[derive(Debug, Clone)]
pub struct Restaurant {
    pub id: RestaurantId,
    pub tenant_id: TenantId,
    pub name: String,
    pub description: Option<String>,
    pub cuisine_type: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    /// Google Places API identifier — populated by the review-sync phase.
    pub google_place_id: Option<String>,
    /// Yelp business identifier — populated by the review-sync phase.
    pub yelp_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
