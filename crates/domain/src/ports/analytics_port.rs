//! Port trait for analytics queries.
//!
//! Returns aggregated statistics computed directly against the live data
//! tables.  Callers (application services) supply a `since` timestamp
//! representing the start of the reporting window.
//!
//! Redis caching (5-min TTL) is applied at the API handler layer, not here.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::{
    identifiers::{RestaurantId, TenantId},
    DomainError,
};

// ---------------------------------------------------------------------------
// Return types
// ---------------------------------------------------------------------------

/// KPI summary combining review and content statistics.
#[derive(Debug, Clone)]
pub struct OverviewData {
    pub total_reviews: i64,
    pub avg_rating: Option<f64>,
    pub avg_sentiment: Option<f64>,
    /// Reviews that have an AI reply draft saved.
    pub reviews_with_reply: i64,
    pub total_content: i64,
    pub published_content: i64,
}

/// Detailed review analytics for a reporting window.
#[derive(Debug, Clone)]
pub struct ReviewsAnalyticsData {
    pub total_reviews: i64,
    pub avg_rating: Option<f64>,
    pub avg_sentiment: Option<f64>,
    pub reviews_with_reply: i64,
    /// Star rating (as string "1"–"5") → review count.
    pub rating_distribution: HashMap<String, i64>,
    /// Platform name ("google", "yelp", "tripadvisor") → review count.
    pub platform_breakdown: HashMap<String, i64>,
}

/// Content-piece analytics for a reporting window.
#[derive(Debug, Clone)]
pub struct ContentAnalyticsData {
    pub total: i64,
    /// Status name ("draft", "approved", "published") → piece count.
    pub by_status: HashMap<String, i64>,
    /// Type name (`social_post`, `email`, …) → piece count.
    pub by_type: HashMap<String, i64>,
}

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

#[async_trait]
pub trait AnalyticsRepository: Send + Sync {
    /// Aggregate overview KPIs for reviews and content since `since`.
    async fn overview(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        since: DateTime<Utc>,
    ) -> Result<OverviewData, DomainError>;

    /// Aggregate review-specific analytics since `since`.
    async fn reviews_analytics(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        since: DateTime<Utc>,
    ) -> Result<ReviewsAnalyticsData, DomainError>;

    /// Aggregate content-piece analytics since `since`.
    async fn content_analytics(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        since: DateTime<Utc>,
    ) -> Result<ContentAnalyticsData, DomainError>;
}
