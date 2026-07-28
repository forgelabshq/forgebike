//! Cursor-based pagination primitives shared by all list endpoints.
//!
//! ## Why cursor-based?
//!
//! Offset pagination (`LIMIT 20 OFFSET 40`) breaks when rows are inserted
//! during traversal — the same item can appear on two consecutive pages, or
//! be skipped entirely.  Cursor-based pagination uses a position anchor
//! (`created_at + id`) that is stable regardless of concurrent writes.
//!
//! ## Cursor encoding
//!
//! The opaque cursor string that the API exposes to callers is handled in
//! `forgebike-api::pagination`.  The domain layer only knows about the
//! decoded [`Cursor`] struct.

use chrono::{DateTime, Utc};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Cursor
// ---------------------------------------------------------------------------

/// A position anchor in an ordered result set.
///
/// Encodes the `(created_at, id)` of the last item on the previous page.
/// Both fields together guarantee uniqueness even if two rows share a
/// timestamp (which can happen when rows are bulk-inserted).
#[derive(Debug, Clone)]
pub struct Cursor {
    pub created_at: DateTime<Utc>,
    pub id: Uuid,
}

impl Cursor {
    /// The "start of everything" sentinel used for the first page.
    ///
    /// `(epoch, nil_uuid)` is less than any real row because:
    /// - All real `created_at` values are after Unix epoch.
    /// - The nil UUID `000…0` sorts before any random v4 UUID.
    #[must_use]
    pub fn start() -> Self {
        Self {
            created_at: DateTime::<Utc>::from_timestamp(0, 0).expect("epoch is a valid timestamp"),
            id: Uuid::nil(),
        }
    }
}

// ---------------------------------------------------------------------------
// ListParams
// ---------------------------------------------------------------------------

/// Input parameters for a paginated list query.
#[derive(Debug, Clone)]
pub struct ListParams {
    /// Maximum items to return per page.  Callers may request up to 100;
    /// the service layer clamps values above that.
    pub limit: i64,
    /// Position to continue from.  `None` → first page.
    pub cursor: Option<Cursor>,
}

impl Default for ListParams {
    fn default() -> Self {
        Self {
            limit: 20,
            cursor: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

/// A single page of results, together with a cursor for the next page.
#[derive(Debug)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Present when there are more items after this page.  Pass this value
    /// back as the `cursor` query parameter to fetch the next page.
    pub next_cursor: Option<Cursor>,
}
