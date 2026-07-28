//! Command and response types for restaurant and menu-item use cases.

// ---------------------------------------------------------------------------
// Restaurant commands
// ---------------------------------------------------------------------------

pub struct CreateRestaurantCommand {
    pub name: String,
    pub description: Option<String>,
    pub cuisine_type: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
}

/// All fields are optional: `None` means "leave the current value unchanged".
pub struct UpdateRestaurantCommand {
    pub name: Option<String>,
    pub description: Option<String>,
    pub cuisine_type: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub google_place_id: Option<String>,
    pub yelp_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Menu item commands
// ---------------------------------------------------------------------------

pub struct CreateMenuItemCommand {
    pub name: String,
    pub description: Option<String>,
    pub price_cents: Option<i64>,
    pub category: Option<String>,
    pub is_available: bool,
}

/// All fields are optional: `None` means "leave the current value unchanged".
pub struct UpdateMenuItemCommand {
    pub name: Option<String>,
    pub description: Option<String>,
    pub price_cents: Option<i64>,
    pub category: Option<String>,
    pub is_available: Option<bool>,
}
