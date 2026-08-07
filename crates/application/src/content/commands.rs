//! Command and query types for the content use cases.

use forgebike_domain::{
    entities::content_piece::{ContentStatus, ContentType},
    pagination::Cursor,
};

// ---------------------------------------------------------------------------
// Generate
// ---------------------------------------------------------------------------

/// Input for the content-generation use case.
pub struct GenerateContentCommand {
    pub content_type: ContentType,
    /// e.g. "summer menu launch", "pasta carbonara", "Mother's Day promo".
    pub topic: Option<String>,
    /// Optional tone / style hint from the restaurant owner.
    pub tone: Option<String>,
}

// ---------------------------------------------------------------------------
// Update
// ---------------------------------------------------------------------------

/// Partial update for a content piece.
///
/// `None` on any field means "leave unchanged".
pub struct UpdateContentCommand {
    pub title: Option<String>,
    pub body: Option<String>,
    pub status: Option<ContentStatus>,
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

pub struct ContentListQuery {
    pub limit: i64,
    pub cursor: Option<Cursor>,
    pub status: Option<ContentStatus>,
    pub content_type: Option<ContentType>,
}
