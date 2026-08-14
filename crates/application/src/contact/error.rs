//! Error type for the contact application service.

use thiserror::Error;

use forgebike_domain::{
    identifiers::{CustomerContactId, RestaurantId},
    DomainError,
};

#[derive(Debug, Error)]
pub enum ContactError {
    #[error("Restaurant {0} not found")]
    RestaurantNotFound(RestaurantId),

    #[error("Contact {0} not found")]
    ContactNotFound(CustomerContactId),

    #[error(transparent)]
    Domain(#[from] DomainError),
}
