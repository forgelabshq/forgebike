//! Error type for the review application service.

use thiserror::Error;

use forgebike_domain::{identifiers::RestaurantId, DomainError};

#[derive(Debug, Error)]
pub enum ReviewError {
    /// The specified restaurant does not exist or belongs to another tenant.
    #[error("Restaurant {0} not found")]
    RestaurantNotFound(RestaurantId),

    /// Bubbled-up infrastructure error.
    #[error(transparent)]
    Domain(#[from] DomainError),
}
