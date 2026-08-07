//! Port trait for review persistence.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::{
    entities::review::{Review, ReviewPlatform},
    identifiers::{RestaurantId, ReviewId, TenantId},
    pagination::{Cursor, Page},
    DomainError,
};

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Data required to insert or update a review row.
///
/// The repository uses `INSERT … ON CONFLICT (restaurant_id, platform,
/// external_id) DO UPDATE` to handle both new reviews and edits.
pub struct UpsertReview {
    pub restaurant_id: RestaurantId,
    pub tenant_id: TenantId,
    pub platform: ReviewPlatform,
    pub external_id: String,
    pub author_name: String,
    pub rating: i16,
    pub body: Option<String>,
    pub published_at: DateTime<Utc>,
}

/// Filters and pagination for the review list query.
///
/// The cursor is descending (`published_at DESC, id DESC`): use
/// [`Cursor::desc_start()`] for the first page.
pub struct ReviewListParams {
    pub limit: i64,
    /// `None` → first page (caller should pass [`Cursor::desc_start()`]).
    pub cursor: Option<Cursor>,
    /// `None` → all platforms.
    pub platform: Option<ReviewPlatform>,
    /// `None` → any rating.
    pub min_rating: Option<i16>,
    /// `None` → no lower date bound.
    pub from_date: Option<DateTime<Utc>>,
    /// `None` → no upper date bound.
    pub to_date: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

#[async_trait]
pub trait ReviewRepository: Send + Sync {
    /// Insert a new review or update the author name, rating, body, and
    /// `published_at` of an existing one (matched by `external_id` + platform).
    async fn upsert(&self, review: UpsertReview) -> Result<Review, DomainError>;

    /// Return a cursor-paginated list of reviews, ordered newest first.
    async fn list(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        params: ReviewListParams,
    ) -> Result<Page<Review>, DomainError>;

    /// Look up a single review by primary key, scoped to the tenant.
    async fn find_by_id(
        &self,
        tenant_id: TenantId,
        id: ReviewId,
    ) -> Result<Option<Review>, DomainError>;

    /// Return reviews that have no sentiment score yet and have a non-empty
    /// body.  Used by the AI sentiment-analysis batch endpoint.
    async fn list_pending_analysis(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        limit: i64,
    ) -> Result<Vec<Review>, DomainError>;

    /// Write the AI-computed sentiment score to a review row.
    async fn update_sentiment(&self, id: ReviewId, score: f32) -> Result<(), DomainError>;

    /// Save an AI-generated reply draft to a review row.
    async fn save_reply_draft(&self, id: ReviewId, draft: &str) -> Result<(), DomainError>;
}
