//! Command and response types for the review use cases.

use chrono::{DateTime, Utc};
use forgebike_domain::{entities::review::ReviewPlatform, pagination::Cursor};

// ---------------------------------------------------------------------------
// Sync
// ---------------------------------------------------------------------------

/// Returned by [`ReviewService::sync_reviews`].
#[derive(Debug)]
pub struct SyncSummary {
    /// Number of reviews upserted (created or updated).
    pub reviews_synced: u32,
    /// Which platform IDs were checked during this sync.
    pub platforms_checked: Vec<String>,
    /// Non-fatal issues — e.g. one platform's API key is missing while
    /// another succeeds.  Callers may surface these as informational warnings.
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

/// Caller-supplied filters for the review list endpoint.
pub struct ReviewQuery {
    pub limit: i64,
    pub cursor: Option<Cursor>,
    pub platform: Option<ReviewPlatform>,
    pub min_rating: Option<i16>,
    pub from_date: Option<DateTime<Utc>>,
    pub to_date: Option<DateTime<Utc>>,
}
