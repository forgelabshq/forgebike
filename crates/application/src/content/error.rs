//! Error type for the content application service.

use thiserror::Error;

use forgebike_domain::{
    identifiers::{ContentPieceId, RestaurantId},
    DomainError,
};

#[derive(Debug, Error)]
pub enum ContentError {
    #[error("Restaurant {0} not found")]
    RestaurantNotFound(RestaurantId),

    #[error("Content piece {0} not found")]
    ContentNotFound(ContentPieceId),

    /// Returned when the `OpenAI` API key is not configured.
    #[error("AI service unavailable: OpenAI API key is not set")]
    AiUnavailable,

    #[error(transparent)]
    Domain(#[from] DomainError),
}
