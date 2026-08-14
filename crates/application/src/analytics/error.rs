//! Error type for the analytics application service.

use thiserror::Error;

use forgebike_domain::{identifiers::RestaurantId, DomainError};

#[derive(Debug, Error)]
pub enum AnalyticsError {
    /// The specified restaurant does not exist or belongs to another tenant.
    #[error("Restaurant {0} not found")]
    RestaurantNotFound(RestaurantId),

    /// `period_days` was outside the accepted range (1–365).
    #[error("Invalid period: {0} days. Accepted values: 30, 90, 365")]
    InvalidPeriod(u32),

    #[error(transparent)]
    Domain(#[from] DomainError),
}
