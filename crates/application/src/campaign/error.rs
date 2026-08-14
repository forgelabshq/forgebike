//! Error type for the campaign application service.

use thiserror::Error;

use forgebike_domain::{
    identifiers::{CampaignId, RestaurantId},
    DomainError,
};

#[derive(Debug, Error)]
pub enum CampaignError {
    #[error("Restaurant {0} not found")]
    RestaurantNotFound(RestaurantId),

    #[error("Campaign {0} not found")]
    CampaignNotFound(CampaignId),

    #[error("Campaign {0} cannot be modified — it is not in draft status")]
    NotDraft(CampaignId),

    #[error("Email is not configured — set APP__EMAIL__SMTP_HOST")]
    EmailNotConfigured,

    #[error("SMS sending is not yet available")]
    SmsNotAvailable,

    #[error(transparent)]
    Domain(#[from] DomainError),
}
