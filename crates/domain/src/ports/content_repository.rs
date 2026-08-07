//! Port trait for content-piece persistence.

use async_trait::async_trait;

use crate::{
    entities::content_piece::{ContentPiece, ContentStatus, ContentType},
    identifiers::{ContentPieceId, RestaurantId, TenantId},
    pagination::{Cursor, Page},
    DomainError,
};

// ---------------------------------------------------------------------------
// Input types
// ---------------------------------------------------------------------------

/// Data required to persist a new content piece.
pub struct NewContentPiece {
    pub restaurant_id: RestaurantId,
    pub tenant_id: TenantId,
    pub content_type: ContentType,
    pub title: Option<String>,
    pub body: String,
}

/// Filters for the content list query.
///
/// The cursor is **descending** (`created_at DESC, id DESC`):
/// use [`Cursor::desc_start()`] for the first page.
pub struct ContentListParams {
    pub limit: i64,
    pub cursor: Option<Cursor>,
    /// `None` → all statuses.
    pub status: Option<ContentStatus>,
    /// `None` → all content types.
    pub content_type: Option<ContentType>,
}

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

#[async_trait]
pub trait ContentRepository: Send + Sync {
    /// Insert a new content piece with status `draft`.
    async fn create(&self, piece: NewContentPiece) -> Result<ContentPiece, DomainError>;

    /// Look up a content piece by primary key, scoped to the tenant.
    async fn find_by_id(
        &self,
        tenant_id: TenantId,
        id: ContentPieceId,
    ) -> Result<Option<ContentPiece>, DomainError>;

    /// Return a cursor-paginated list of content pieces, newest first.
    async fn list(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        params: ContentListParams,
    ) -> Result<Page<ContentPiece>, DomainError>;

    /// Persist all mutable fields (title, body, status) of an existing piece.
    async fn update(&self, piece: &ContentPiece) -> Result<ContentPiece, DomainError>;

    /// Delete a content piece.  Returns `true` if deleted, `false` if not found.
    async fn delete(&self, tenant_id: TenantId, id: ContentPieceId) -> Result<bool, DomainError>;
}
