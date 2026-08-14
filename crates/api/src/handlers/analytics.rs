//! Business-intelligence analytics handlers.
//!
//! Three read-only endpoints aggregate KPI data from live `reviews` and
//! `content_pieces` tables and cache the result in Redis for 5 minutes.
//!
//! ## Endpoints
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | GET | `/api/v1/restaurants/:id/analytics/overview` | KPI summary |
//! | GET | `/api/v1/restaurants/:id/analytics/reviews` | Review analytics |
//! | GET | `/api/v1/restaurants/:id/analytics/content` | Content analytics |
//!
//! ## Caching
//! Each endpoint caches its JSON response in Redis with a 5-minute TTL.
//! The cache key encodes the tenant, restaurant, endpoint, and period so
//! cross-tenant isolation is preserved without extra checks.
//!
//! ## Period
//! The `?period=` query parameter accepts `30`, `90`, or `365` (days).
//! Any other value returns `422 Unprocessable Entity`.  Defaults to `30`.

use std::collections::HashMap;

use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    Extension, Json,
};
use deadpool_redis::redis::AsyncCommands as _;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgebike_domain::{entities::auth_identity::AuthIdentity, identifiers::RestaurantId};

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct PeriodQuery {
    /// Reporting window in days.  Must be one of `30`, `90`, `365`.
    #[serde(default = "default_period")]
    pub period: u32,
}

fn default_period() -> u32 {
    30
}

// ---------------------------------------------------------------------------
// Response DTOs (Serialize + Deserialize so they can round-trip through Redis)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
pub struct OverviewResponse {
    pub period_days: u32,
    pub total_reviews: i64,
    pub avg_rating: Option<f64>,
    pub avg_sentiment: Option<f64>,
    pub reviews_with_reply: i64,
    pub total_content: i64,
    pub published_content: i64,
}

#[derive(Serialize, Deserialize)]
pub struct ReviewsAnalyticsResponse {
    pub period_days: u32,
    pub total_reviews: i64,
    pub avg_rating: Option<f64>,
    pub avg_sentiment: Option<f64>,
    pub reviews_with_reply: i64,
    pub rating_distribution: HashMap<String, i64>,
    pub platform_breakdown: HashMap<String, i64>,
}

#[derive(Serialize, Deserialize)]
pub struct ContentAnalyticsResponse {
    pub period_days: u32,
    pub total: i64,
    pub by_status: HashMap<String, i64>,
    pub by_type: HashMap<String, i64>,
}

// ---------------------------------------------------------------------------
// Redis cache helpers
// ---------------------------------------------------------------------------

/// Cache TTL: 5 minutes.
const CACHE_TTL_SECS: u64 = 5 * 60;

/// Try to retrieve a cached JSON string.  Returns `None` on any error.
async fn cache_get(state: &AppState, key: &str) -> Option<String> {
    let mut conn = state.redis.get().await.ok()?;
    conn.get::<_, Option<String>>(key).await.ok().flatten()
}

/// Silently store a JSON string in Redis with a 5-minute TTL.
async fn cache_set(state: &AppState, key: &str, value: &str) {
    if let Ok(mut conn) = state.redis.get().await {
        let _: Result<(), _> = conn.set_ex::<_, _, ()>(key, value, CACHE_TTL_SECS).await;
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/restaurants/:id/analytics/overview`
///
/// Returns a KPI summary combining review and content statistics for the
/// specified reporting window (`period` defaults to the last 30 days).
#[tracing::instrument(skip(state), name = "handlers::analytics::overview")]
pub async fn overview(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    Query(q): Query<PeriodQuery>,
) -> ApiResult<impl IntoResponse> {
    let rid = RestaurantId::from_uuid(restaurant_id);
    let cache_key = format!(
        "analytics:overview:{}:{}:{}",
        identity.tenant_id, rid, q.period
    );

    // --- Cache hit? -------------------------------------------------------
    if let Some(cached) = cache_get(&state, &cache_key).await {
        if let Ok(resp) = serde_json::from_str::<OverviewResponse>(&cached) {
            return Ok(Json(resp));
        }
    }

    // --- Compute ----------------------------------------------------------
    let data = state
        .analytics_service
        .overview(&identity, rid, q.period)
        .await
        .map_err(ApiError::from)?;

    let resp = OverviewResponse {
        period_days: q.period,
        total_reviews: data.total_reviews,
        avg_rating: data.avg_rating,
        avg_sentiment: data.avg_sentiment,
        reviews_with_reply: data.reviews_with_reply,
        total_content: data.total_content,
        published_content: data.published_content,
    };

    // --- Cache & respond --------------------------------------------------
    if let Ok(body) = serde_json::to_string(&resp) {
        cache_set(&state, &cache_key, &body).await;
    }

    Ok(Json(resp))
}

/// `GET /api/v1/restaurants/:id/analytics/reviews`
///
/// Detailed review analytics: rating distribution, platform breakdown,
/// average sentiment and rating.
#[tracing::instrument(skip(state), name = "handlers::analytics::reviews_analytics")]
pub async fn reviews_analytics(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    Query(q): Query<PeriodQuery>,
) -> ApiResult<impl IntoResponse> {
    let rid = RestaurantId::from_uuid(restaurant_id);
    let cache_key = format!(
        "analytics:reviews:{}:{}:{}",
        identity.tenant_id, rid, q.period
    );

    if let Some(cached) = cache_get(&state, &cache_key).await {
        if let Ok(resp) = serde_json::from_str::<ReviewsAnalyticsResponse>(&cached) {
            return Ok(Json(resp));
        }
    }

    let data = state
        .analytics_service
        .reviews(&identity, rid, q.period)
        .await
        .map_err(ApiError::from)?;

    let resp = ReviewsAnalyticsResponse {
        period_days: q.period,
        total_reviews: data.total_reviews,
        avg_rating: data.avg_rating,
        avg_sentiment: data.avg_sentiment,
        reviews_with_reply: data.reviews_with_reply,
        rating_distribution: data.rating_distribution,
        platform_breakdown: data.platform_breakdown,
    };

    if let Ok(body) = serde_json::to_string(&resp) {
        cache_set(&state, &cache_key, &body).await;
    }

    Ok(Json(resp))
}

/// `GET /api/v1/restaurants/:id/analytics/content`
///
/// Content-piece analytics: total pieces, breakdown by status and type.
#[tracing::instrument(skip(state), name = "handlers::analytics::content_analytics")]
pub async fn content_analytics(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    Query(q): Query<PeriodQuery>,
) -> ApiResult<impl IntoResponse> {
    let rid = RestaurantId::from_uuid(restaurant_id);
    let cache_key = format!(
        "analytics:content:{}:{}:{}",
        identity.tenant_id, rid, q.period
    );

    if let Some(cached) = cache_get(&state, &cache_key).await {
        if let Ok(resp) = serde_json::from_str::<ContentAnalyticsResponse>(&cached) {
            return Ok(Json(resp));
        }
    }

    let data = state
        .analytics_service
        .content(&identity, rid, q.period)
        .await
        .map_err(ApiError::from)?;

    let resp = ContentAnalyticsResponse {
        period_days: q.period,
        total: data.total,
        by_status: data.by_status,
        by_type: data.by_type,
    };

    if let Ok(body) = serde_json::to_string(&resp) {
        cache_set(&state, &cache_key, &body).await;
    }

    Ok(Json(resp))
}
