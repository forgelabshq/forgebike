//! Error type for the restaurant application service.

use thiserror::Error;

use forgebike_domain::{
    identifiers::{MenuItemId, RestaurantId},
    DomainError,
};

#[derive(Debug, Error)]
pub enum RestaurantError {
    /// The requested restaurant does not exist or belongs to another tenant.
    #[error("Restaurant {0} not found")]
    RestaurantNotFound(RestaurantId),

    /// The requested menu item does not exist or belongs to another tenant.
    #[error("Menu item {0} not found")]
    MenuItemNotFound(MenuItemId),

    /// The menu item does not belong to the specified restaurant.
    #[error("Menu item {0} does not belong to restaurant {1}")]
    WrongRestaurant(MenuItemId, RestaurantId),

    /// Bubbled-up infrastructure error.
    #[error(transparent)]
    Domain(#[from] DomainError),
}
