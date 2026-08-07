//! `sqlx`-backed implementation of [`ContentRepository`].
//!
//! Content pieces are listed newest-first, using the same descending cursor
//! pattern as reviews.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use forgebike_domain::{
    entities::content_piece::{ContentPiece, ContentStatus, ContentType},
    identifiers::{ContentPieceId, RestaurantId, TenantId},
    pagination::{Cursor, Page},
    ports::content_repository::{ContentListParams, ContentRepository, NewContentPiece},
    DomainError,
};

// ---------------------------------------------------------------------------
// DB row
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ContentRow {
    id: Uuid,
    restaurant_id: Uuid,
    tenant_id: Uuid,
    content_type: String,
    title: Option<String>,
    body: String,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ContentRow> for ContentPiece {
    type Error = DomainError;

    fn try_from(r: ContentRow) -> Result<Self, Self::Error> {
        Ok(ContentPiece {
            id: ContentPieceId::from_uuid(r.id),
            restaurant_id: RestaurantId::from_uuid(r.restaurant_id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            content_type: r
                .content_type
                .parse::<ContentType>()
                .map_err(DomainError::Internal)?,
            title: r.title,
            body: r.body,
            status: r
                .status
                .parse::<ContentStatus>()
                .map_err(DomainError::Internal)?,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

pub struct PgContentRepository {
    pool: PgPool,
}

impl PgContentRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ContentRepository for PgContentRepository {
    async fn create(&self, p: NewContentPiece) -> Result<ContentPiece, DomainError> {
        let row = sqlx::query_as::<_, ContentRow>(
            r"
            INSERT INTO content_pieces
                (restaurant_id, tenant_id, content_type, title, body)
            VALUES ($1, $2, $3::content_type, $4, $5)
            RETURNING
                id, restaurant_id, tenant_id,
                content_type::TEXT AS content_type,
                title, body, status::TEXT AS status,
                created_at, updated_at
            ",
        )
        .bind(p.restaurant_id.as_uuid())
        .bind(p.tenant_id.as_uuid())
        .bind(p.content_type.to_string())
        .bind(&p.title)
        .bind(&p.body)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        ContentPiece::try_from(row)
    }

    async fn find_by_id(
        &self,
        tenant_id: TenantId,
        id: ContentPieceId,
    ) -> Result<Option<ContentPiece>, DomainError> {
        let row = sqlx::query_as::<_, ContentRow>(
            r"
            SELECT
                id, restaurant_id, tenant_id,
                content_type::TEXT AS content_type,
                title, body, status::TEXT AS status,
                created_at, updated_at
            FROM content_pieces
            WHERE tenant_id = $1 AND id = $2
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        row.map(ContentPiece::try_from).transpose()
    }

    async fn list(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        params: ContentListParams,
    ) -> Result<Page<ContentPiece>, DomainError> {
        let cursor = params.cursor.unwrap_or_else(Cursor::desc_start);
        let fetch_limit = params.limit + 1;

        let rows = sqlx::query_as::<_, ContentRow>(
            r"
            SELECT
                id, restaurant_id, tenant_id,
                content_type::TEXT AS content_type,
                title, body, status::TEXT AS status,
                created_at, updated_at
            FROM content_pieces
            WHERE tenant_id     = $1
              AND restaurant_id = $2
              AND ($3::TEXT IS NULL OR status::TEXT       = $3)
              AND ($4::TEXT IS NULL OR content_type::TEXT = $4)
              AND (created_at < $5
                   OR (created_at = $5 AND id < $6))
            ORDER BY created_at DESC, id DESC
            LIMIT $7
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(params.status.as_ref().map(std::string::ToString::to_string))
        .bind(
            params
                .content_type
                .as_ref()
                .map(std::string::ToString::to_string),
        )
        .bind(cursor.created_at)
        .bind(cursor.id)
        .bind(fetch_limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        build_page(rows, params.limit)
    }

    async fn update(&self, p: &ContentPiece) -> Result<ContentPiece, DomainError> {
        let row = sqlx::query_as::<_, ContentRow>(
            r"
            UPDATE content_pieces
            SET title        = $3,
                body         = $4,
                status       = $5::content_status
            WHERE id = $1 AND tenant_id = $2
            RETURNING
                id, restaurant_id, tenant_id,
                content_type::TEXT AS content_type,
                title, body, status::TEXT AS status,
                created_at, updated_at
            ",
        )
        .bind(p.id.as_uuid())
        .bind(p.tenant_id.as_uuid())
        .bind(&p.title)
        .bind(&p.body)
        .bind(p.status.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        ContentPiece::try_from(row)
    }

    async fn delete(&self, tenant_id: TenantId, id: ContentPieceId) -> Result<bool, DomainError> {
        let result = sqlx::query("DELETE FROM content_pieces WHERE id = $1 AND tenant_id = $2")
            .bind(id.as_uuid())
            .bind(tenant_id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }
}

// ---------------------------------------------------------------------------
// Pagination helper (descending — same pattern as reviews)
// ---------------------------------------------------------------------------

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::unnecessary_wraps
)]
fn build_page(mut rows: Vec<ContentRow>, limit: i64) -> Result<Page<ContentPiece>, DomainError> {
    let limit_usize = usize::try_from(limit).unwrap_or(usize::MAX);
    let has_more = rows.len() > limit_usize;
    if has_more {
        rows.truncate(limit_usize);
    }

    let next_cursor = if has_more {
        rows.last().map(|r| Cursor {
            created_at: r.created_at,
            id: r.id,
        })
    } else {
        None
    };

    Ok(Page {
        items: rows
            .into_iter()
            .map(ContentPiece::try_from)
            .collect::<Result<_, _>>()?,
        next_cursor,
    })
}
