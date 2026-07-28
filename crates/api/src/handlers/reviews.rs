//! Review handlers — sync and list.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgebike_application::review::commands::ReviewQuery;
use forgebike_domain::{
    entities::{auth_identity::AuthIdentity, review::Review},
    identifiers::RestaurantId,
};

use crate::{
    error::{ApiError, ApiResult},
    pagination::{decode_cursor, encode_cursor, PageResponse},
    state::AppState,
};

// ---------------------------------------------------------------------------
// Response DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ReviewResponse {
    pub id: String,
    pub platform: String,
    pub external_id: String,
    pub author_name: String,
    pub rating: i16,
    pub body: Option<String>,
    pub published_at: String,
    pub sentiment_score: Option<f32>,
    pub ai_reply_draft: Option<String>,
    pub created_at: String,
}

impl From<Review> for ReviewResponse {
    fn from(r: Review) -> Self {
        Self {
            id: r.id.to_string(),
            platform: r.platform.to_string(),
            external_id: r.external_id,
            author_name: r.author_name,
            rating: r.rating,
            body: r.body,
            published_at: r.published_at.to_rfc3339(),
            sentiment_score: r.sentiment_score,
            ai_reply_draft: r.ai_reply_draft,
            created_at: r.created_at.to_rfc3339(),
        }
    }
}

#[derive(Serialize)]
pub struct SyncResponse {
    pub reviews_synced: u32,
    pub platforms_checked: Vec<String>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Query params for the list endpoint
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ReviewListQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub cursor: Option<String>,
    /// Filter by platform: `google` | `yelp` | `tripadvisor`
    pub platform: Option<String>,
    /// Only return reviews with this rating or higher (1–5).
    pub min_rating: Option<i16>,
    /// Only return reviews published on or after this date (RFC 3339).
    pub from: Option<DateTime<Utc>>,
    /// Only return reviews published on or before this date (RFC 3339).
    pub to: Option<DateTime<Utc>>,
}

fn default_limit() -> i64 {
    20
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/restaurants/:id/reviews/sync`
///
/// Fetches reviews from all configured external platforms for the restaurant
/// and upserts them into the database.  Runs synchronously and returns the
/// sync summary when complete.
#[tracing::instrument(skip(state), name = "handlers::reviews::sync")]
pub async fn sync_reviews(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let summary = state
        .review_service
        .sync_reviews(&identity, RestaurantId::from_uuid(restaurant_id))
        .await
        .map_err(ApiError::from)?;

    Ok(Json(SyncResponse {
        reviews_synced: summary.reviews_synced,
        platforms_checked: summary.platforms_checked,
        warnings: summary.warnings,
    }))
}

/// `GET /api/v1/restaurants/:id/reviews`
///
/// Returns a cursor-paginated, newest-first list of reviews.
/// Supports optional filters: `platform`, `min_rating`, `from`, `to`.
#[tracing::instrument(skip(state), name = "handlers::reviews::list")]
pub async fn list_reviews(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    Query(q): Query<ReviewListQuery>,
) -> ApiResult<impl IntoResponse> {
    let platform = q
        .platform
        .as_deref()
        .map(|s| {
            s.parse::<forgebike_domain::entities::review::ReviewPlatform>()
                .map_err(|_| {
                    ApiError::new(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        format!("unknown platform: {s}"),
                    )
                })
        })
        .transpose()?;

    let page = state
        .review_service
        .list_reviews(
            &identity,
            RestaurantId::from_uuid(restaurant_id),
            ReviewQuery {
                limit: q.limit.clamp(1, 100),
                cursor: q.cursor.as_deref().and_then(decode_cursor),
                platform,
                min_rating: q.min_rating,
                from_date: q.from,
                to_date: q.to,
            },
        )
        .await
        .map_err(ApiError::from)?;

    let next_cursor = page.next_cursor.as_ref().map(encode_cursor);
    let items: Vec<ReviewResponse> = page.items.into_iter().map(Into::into).collect();

    Ok(Json(PageResponse::new(items, next_cursor)))
}
