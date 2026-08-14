//! `sqlx`-backed implementation of [`AnalyticsRepository`].
//!
//! All queries run as real-time aggregations against the live `reviews` and
//! `content_pieces` tables.  Callers should cache results externally (the API
//! handlers apply a 5-minute Redis TTL).
//!
//! ## Why `query_as` and not `query!`
//! Every other repository in this codebase uses `sqlx::query_as::<_, T>()`
//! with explicit `#[derive(sqlx::FromRow)]` row structs so that the workspace
//! compiles with `SQLX_OFFLINE=true` in CI without requiring a `.sqlx` cache
//! file.  This file follows the same pattern.
//!
//! ## SQL notes
//! - `AVG(col::DOUBLE PRECISION)` — explicit cast avoids a `NUMERIC` return
//!   type, giving us `Option<f64>` in Rust (NULL for an empty set).
//! - `COUNT(*) FILTER (WHERE …)` — `PostgreSQL` 9.4+ conditional aggregates;
//!   always returns `bigint` (never NULL), so the Rust field is `i64`.
//! - `enum_col::TEXT` — casts a `PostgreSQL` enum to its text label; matches
//!   the pattern used in `review_repository.rs` and `content_repository.rs`.

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use forgebike_domain::{
    identifiers::{RestaurantId, TenantId},
    ports::analytics_port::{
        AnalyticsRepository, ContentAnalyticsData, OverviewData, ReviewsAnalyticsData,
    },
    DomainError,
};

// ---------------------------------------------------------------------------
// Row types — one struct per distinct result shape
// ---------------------------------------------------------------------------

/// Aggregated review KPIs from a single `SELECT … FROM reviews WHERE …`.
#[derive(sqlx::FromRow)]
struct ReviewAggRow {
    total_reviews: i64,
    avg_rating: Option<f64>,
    avg_sentiment: Option<f64>,
    reviews_with_reply: i64,
}

/// Aggregated content KPIs from a single `SELECT … FROM content_pieces WHERE …`.
#[derive(sqlx::FromRow)]
struct ContentAggRow {
    total: i64,
    published: i64,
}

/// One row from a `GROUP BY rating` query.
#[derive(sqlx::FromRow)]
struct RatingCountRow {
    rating: i16,
    cnt: i64,
}

/// One row from a `GROUP BY platform` query.
#[derive(sqlx::FromRow)]
struct PlatformCountRow {
    platform: String,
    cnt: i64,
}

/// One row from a `GROUP BY status` query on `content_pieces`.
#[derive(sqlx::FromRow)]
struct StatusCountRow {
    status: String,
    cnt: i64,
}

/// One row from a `GROUP BY content_type` query on `content_pieces`.
#[derive(sqlx::FromRow)]
struct TypeCountRow {
    content_type: String,
    cnt: i64,
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

pub struct PgAnalyticsRepository {
    pool: PgPool,
}

impl PgAnalyticsRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// ---------------------------------------------------------------------------
// Port implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl AnalyticsRepository for PgAnalyticsRepository {
    // -----------------------------------------------------------------------
    // Overview
    // -----------------------------------------------------------------------

    async fn overview(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        since: DateTime<Utc>,
    ) -> Result<OverviewData, DomainError> {
        // --- Reviews -------------------------------------------------------
        let rev = sqlx::query_as::<_, ReviewAggRow>(
            r"
            SELECT
                COUNT(*)                                                       AS total_reviews,
                AVG(rating::DOUBLE PRECISION)                                  AS avg_rating,
                AVG(sentiment_score::DOUBLE PRECISION)                         AS avg_sentiment,
                COUNT(*) FILTER (WHERE ai_reply_draft IS NOT NULL)             AS reviews_with_reply
            FROM reviews
            WHERE tenant_id = $1 AND restaurant_id = $2 AND published_at >= $3
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(since)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        // --- Content -------------------------------------------------------
        let con = sqlx::query_as::<_, ContentAggRow>(
            r"
            SELECT
                COUNT(*)                                                           AS total,
                COUNT(*) FILTER (WHERE status = 'published'::content_status)      AS published
            FROM content_pieces
            WHERE tenant_id = $1 AND restaurant_id = $2 AND created_at >= $3
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(since)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(OverviewData {
            total_reviews: rev.total_reviews,
            avg_rating: rev.avg_rating,
            avg_sentiment: rev.avg_sentiment,
            reviews_with_reply: rev.reviews_with_reply,
            total_content: con.total,
            published_content: con.published,
        })
    }

    // -----------------------------------------------------------------------
    // Reviews analytics
    // -----------------------------------------------------------------------

    async fn reviews_analytics(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        since: DateTime<Utc>,
    ) -> Result<ReviewsAnalyticsData, DomainError> {
        // --- Aggregate totals ----------------------------------------------
        let agg = sqlx::query_as::<_, ReviewAggRow>(
            r"
            SELECT
                COUNT(*)                                                       AS total_reviews,
                AVG(rating::DOUBLE PRECISION)                                  AS avg_rating,
                AVG(sentiment_score::DOUBLE PRECISION)                         AS avg_sentiment,
                COUNT(*) FILTER (WHERE ai_reply_draft IS NOT NULL)             AS reviews_with_reply
            FROM reviews
            WHERE tenant_id = $1 AND restaurant_id = $2 AND published_at >= $3
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(since)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        // --- Rating distribution -------------------------------------------
        let rating_rows = sqlx::query_as::<_, RatingCountRow>(
            r"
            SELECT rating, COUNT(*) AS cnt
            FROM reviews
            WHERE tenant_id = $1 AND restaurant_id = $2 AND published_at >= $3
            GROUP BY rating
            ORDER BY rating
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        let rating_distribution: HashMap<String, i64> = rating_rows
            .into_iter()
            .map(|r| (r.rating.to_string(), r.cnt))
            .collect();

        // --- Platform breakdown --------------------------------------------
        let platform_rows = sqlx::query_as::<_, PlatformCountRow>(
            r"
            SELECT platform::TEXT AS platform, COUNT(*) AS cnt
            FROM reviews
            WHERE tenant_id = $1 AND restaurant_id = $2 AND published_at >= $3
            GROUP BY platform
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        let platform_breakdown: HashMap<String, i64> = platform_rows
            .into_iter()
            .map(|r| (r.platform, r.cnt))
            .collect();

        Ok(ReviewsAnalyticsData {
            total_reviews: agg.total_reviews,
            avg_rating: agg.avg_rating,
            avg_sentiment: agg.avg_sentiment,
            reviews_with_reply: agg.reviews_with_reply,
            rating_distribution,
            platform_breakdown,
        })
    }

    // -----------------------------------------------------------------------
    // Content analytics
    // -----------------------------------------------------------------------

    async fn content_analytics(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        since: DateTime<Utc>,
    ) -> Result<ContentAnalyticsData, DomainError> {
        // --- By status -----------------------------------------------------
        let status_rows = sqlx::query_as::<_, StatusCountRow>(
            r"
            SELECT status::TEXT AS status, COUNT(*) AS cnt
            FROM content_pieces
            WHERE tenant_id = $1 AND restaurant_id = $2 AND created_at >= $3
            GROUP BY status
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        let by_status: HashMap<String, i64> =
            status_rows.into_iter().map(|r| (r.status, r.cnt)).collect();

        let total: i64 = by_status.values().sum();

        // --- By content type -----------------------------------------------
        let type_rows = sqlx::query_as::<_, TypeCountRow>(
            r"
            SELECT content_type::TEXT AS content_type, COUNT(*) AS cnt
            FROM content_pieces
            WHERE tenant_id = $1 AND restaurant_id = $2 AND created_at >= $3
            GROUP BY content_type
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        let by_type: HashMap<String, i64> = type_rows
            .into_iter()
            .map(|r| (r.content_type, r.cnt))
            .collect();

        Ok(ContentAnalyticsData {
            total,
            by_status,
            by_type,
        })
    }
}
