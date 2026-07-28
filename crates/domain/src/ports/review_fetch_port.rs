//! Port trait for fetching reviews from an external platform.
//!
//! The infrastructure layer provides concrete implementations
//! (`GooglePlacesClient`, `YelpFusionClient`, `TripAdvisorClient`).
//! Tests use simple in-memory mocks.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::DomainError;

// ---------------------------------------------------------------------------
// Fetched review (platform-agnostic output from each client)
// ---------------------------------------------------------------------------

/// A single review returned by an external platform client.
///
/// The concrete HTTP response format varies by platform; each client maps
/// its native response shape into this common struct before returning it.
#[derive(Debug, Clone)]
pub struct FetchedReview {
    /// Platform-assigned unique identifier for this review.
    pub external_id: String,
    pub author_name: String,
    /// Star rating (1–5).
    pub rating: i16,
    pub body: Option<String>,
    pub published_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

#[async_trait]
pub trait ReviewFetchPort: Send + Sync {
    /// Fetch recent reviews for the given platform-specific location identifier.
    ///
    /// Returns `Ok(vec![])` when the client's API key is not configured —
    /// this is treated as a graceful skip rather than an error by the service.
    async fn fetch_reviews(&self, external_id: &str) -> Result<Vec<FetchedReview>, DomainError>;
}
