//! Port trait for campaign persistence.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::{
    entities::campaign::{Campaign, CampaignChannel, CampaignStatus},
    identifiers::{CampaignId, RestaurantId, TenantId},
    pagination::{Cursor, Page},
    DomainError,
};

// ---------------------------------------------------------------------------
// Command / query types
// ---------------------------------------------------------------------------

/// Data required to create a new campaign.
#[derive(Debug, Clone)]
pub struct NewCampaign {
    pub tenant_id: TenantId,
    pub restaurant_id: RestaurantId,
    pub name: String,
    pub channel: CampaignChannel,
    pub subject: Option<String>,
    pub body: String,
    pub tag_filter: Option<String>,
    pub scheduled_at: Option<DateTime<Utc>>,
}

/// Fields that can be changed on a `draft` campaign.  `None` = leave as-is.
#[derive(Debug, Clone, Default)]
pub struct UpdateCampaign {
    pub name: Option<String>,
    pub subject: Option<Option<String>>,
    pub body: Option<String>,
    pub tag_filter: Option<Option<String>>,
    pub scheduled_at: Option<Option<DateTime<Utc>>>,
}

/// Parameters for the paginated campaign list.
#[derive(Debug, Clone)]
pub struct CampaignListParams {
    pub limit: i64,
    pub cursor: Option<Cursor>,
    pub status: Option<CampaignStatus>,
}

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

#[async_trait]
pub trait CampaignRepository: Send + Sync {
    async fn create(&self, campaign: NewCampaign) -> Result<Campaign, DomainError>;

    async fn find_by_id(
        &self,
        tenant_id: TenantId,
        id: CampaignId,
    ) -> Result<Option<Campaign>, DomainError>;

    async fn list(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        params: CampaignListParams,
    ) -> Result<Page<Campaign>, DomainError>;

    /// Partial update — only allowed for `draft` campaigns.
    async fn update(
        &self,
        tenant_id: TenantId,
        id: CampaignId,
        update: UpdateCampaign,
    ) -> Result<Option<Campaign>, DomainError>;

    /// Delete a campaign — only allowed for `draft` campaigns at the service
    /// layer (the repo performs the unconditional DELETE).
    async fn delete(&self, tenant_id: TenantId, id: CampaignId) -> Result<bool, DomainError>;

    /// Atomically transition the status field (and optionally `recipients_count`
    /// / `sent_at`) — used by the send background task.
    async fn set_status(
        &self,
        id: CampaignId,
        status: CampaignStatus,
        recipients_count: Option<i32>,
        sent_at: Option<DateTime<Utc>>,
    ) -> Result<(), DomainError>;
}
