//! Concrete [`ReviewFetchPort`] implementations for each external platform.
//!
//! Each client reads its API key from the application configuration.
//! When the key is empty the client returns `Ok(vec![])` rather than an
//! error, so the sync service can gracefully skip unconfigured platforms.

pub mod google;
pub mod tripadvisor;
pub mod yelp;

pub use google::GooglePlacesClient;
pub use tripadvisor::TripAdvisorClient;
pub use yelp::YelpFusionClient;
