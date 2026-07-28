//! `sqlx`-backed implementation of [`ReviewRepository`].
//!
//! ## Deduplication
//! Uses `INSERT … ON CONFLICT (restaurant_id, platform, external_id) DO UPDATE`
//! so that re-syncing the same review updates the author name, rating, and
//! body rather than creating a duplicate row.
//!
//! ## Cursor (descending)
//! Reviews are listed newest-first.  The cursor encodes `(published_at, id)`;
//! the WHERE clause is `(published_at, id) < ($cursor_ts, $cursor_id)`.
//! For the first page, call [`Cursor::desc_start()`].

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use forgebike_domain::{
    entities::review::{Review, ReviewPlatform},
    identifiers::{RestaurantId, ReviewId, TenantId},
    pagination::{Cursor, Page},
    ports::review_repository::{ReviewListParams, ReviewRepository, UpsertReview},
    DomainError,
};

// ---------------------------------------------------------------------------
// DB row
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ReviewRow {
    id: Uuid,
    restaurant_id: Uuid,
    tenant_id: Uuid,
    platform: String,
    external_id: String,
    author_name: String,
    rating: i16,
    body: Option<String>,
    published_at: DateTime<Utc>,
    sentiment_score: Option<f32>,
    ai_reply_draft: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ReviewRow> for Review {
    type Error = DomainError;

    fn try_from(r: ReviewRow) -> Result<Self, Self::Error> {
        let platform = r
            .platform
            .parse::<ReviewPlatform>()
            .map_err(DomainError::Internal)?;

        Ok(Review {
            id: ReviewId::from_uuid(r.id),
            restaurant_id: RestaurantId::from_uuid(r.restaurant_id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            platform,
            external_id: r.external_id,
            author_name: r.author_name,
            rating: r.rating,
            body: r.body,
            published_at: r.published_at,
            sentiment_score: r.sentiment_score,
            ai_reply_draft: r.ai_reply_draft,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

pub struct PgReviewRepository {
    pool: PgPool,
}

impl PgReviewRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ReviewRepository for PgReviewRepository {
    async fn upsert(&self, r: UpsertReview) -> Result<Review, DomainError> {
        let row = sqlx::query_as::<_, ReviewRow>(
            r"
            INSERT INTO reviews
                (restaurant_id, tenant_id, platform, external_id,
                 author_name, rating, body, published_at)
            VALUES ($1, $2, $3::review_platform, $4, $5, $6, $7, $8)
            ON CONFLICT (restaurant_id, platform, external_id) DO UPDATE
                SET author_name  = EXCLUDED.author_name,
                    rating       = EXCLUDED.rating,
                    body         = EXCLUDED.body,
                    published_at = EXCLUDED.published_at
            RETURNING
                id, restaurant_id, tenant_id,
                platform::TEXT AS platform,
                external_id, author_name, rating, body, published_at,
                sentiment_score, ai_reply_draft, created_at, updated_at
            ",
        )
        .bind(r.restaurant_id.as_uuid())
        .bind(r.tenant_id.as_uuid())
        .bind(r.platform.to_string())
        .bind(&r.external_id)
        .bind(&r.author_name)
        .bind(r.rating)
        .bind(&r.body)
        .bind(r.published_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        Review::try_from(row)
    }

    async fn list(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        params: ReviewListParams,
    ) -> Result<Page<Review>, DomainError> {
        // Descending cursor: the sentinel for the first page is desc_start().
        let cursor = params.cursor.unwrap_or_else(Cursor::desc_start);
        let fetch_limit = params.limit + 1;

        // Filters are applied conditionally using IS NULL short-circuits.
        let rows = sqlx::query_as::<_, ReviewRow>(
            r"
            SELECT
                id, restaurant_id, tenant_id,
                platform::TEXT AS platform,
                external_id, author_name, rating, body, published_at,
                sentiment_score, ai_reply_draft, created_at, updated_at
            FROM reviews
            WHERE tenant_id     = $1
              AND restaurant_id = $2
              AND ($3::TEXT        IS NULL OR platform::TEXT = $3)
              AND ($4::SMALLINT   IS NULL OR rating >= $4)
              AND ($5::TIMESTAMPTZ IS NULL OR published_at >= $5)
              AND ($6::TIMESTAMPTZ IS NULL OR published_at <= $6)
              AND (published_at < $7
                   OR (published_at = $7 AND id < $8))
            ORDER BY published_at DESC, id DESC
            LIMIT $9
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(
            params
                .platform
                .as_ref()
                .map(std::string::ToString::to_string),
        )
        .bind(params.min_rating)
        .bind(params.from_date)
        .bind(params.to_date)
        .bind(cursor.created_at) // $7 — published_at cursor
        .bind(cursor.id) // $8 — id cursor
        .bind(fetch_limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        build_page(rows, params.limit)
    }
}

// ---------------------------------------------------------------------------
// Pagination helper
// ---------------------------------------------------------------------------

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::unnecessary_wraps
)]
fn build_page(mut rows: Vec<ReviewRow>, limit: i64) -> Result<Page<Review>, DomainError> {
    let limit_usize = usize::try_from(limit).unwrap_or(usize::MAX);
    let has_more = rows.len() > limit_usize;
    if has_more {
        rows.truncate(limit_usize);
    }

    // For descending pages the cursor is the LAST item returned (smallest
    // published_at on this page — the next page continues below it).
    let next_cursor = if has_more {
        rows.last().map(|r| Cursor {
            created_at: r.published_at,
            id: r.id,
        })
    } else {
        None
    };

    Ok(Page {
        items: rows
            .into_iter()
            .map(Review::try_from)
            .collect::<Result<_, _>>()?,
        next_cursor,
    })
}
