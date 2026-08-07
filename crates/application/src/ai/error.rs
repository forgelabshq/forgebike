//! Error type for the AI application service.

use thiserror::Error;

use forgebike_domain::{
    identifiers::{RestaurantId, ReviewId},
    DomainError,
};

#[derive(Debug, Error)]
pub enum AiError {
    /// The specified restaurant does not exist or belongs to another tenant.
    #[error("Restaurant {0} not found")]
    RestaurantNotFound(RestaurantId),

    /// The specified review does not exist or belongs to another tenant.
    #[error("Review {0} not found")]
    ReviewNotFound(ReviewId),

    /// The review has no body text to analyse or reply to.
    #[error("Review {0} has no text to process")]
    NoReviewText(ReviewId),

    /// The `OpenAI` API key is not configured.
    #[error("AI service unavailable: OpenAI API key is not set")]
    AiUnavailable,

    /// Bubbled-up infrastructure error.
    #[error(transparent)]
    Domain(#[from] DomainError),
}
