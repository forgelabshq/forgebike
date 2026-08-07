//! AI handlers — sentiment analysis, reply drafts, and reply publishing.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgebike_domain::{
    entities::{auth_identity::AuthIdentity, review::Review},
    identifiers::{RestaurantId, ReviewId},
};

use crate::{
    error::{ApiError, ApiResult},
    state::AppState,
};

// ---------------------------------------------------------------------------
// Response DTOs  (reuse ReviewResponse shape from reviews handler)
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
pub struct AnalysisResponse {
    pub analysed: u32,
    pub skipped: u32,
    pub tokens_used: u64,
}

#[derive(Serialize)]
pub struct ReplyDraftResponse {
    pub review_id: String,
    pub draft: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/restaurants/:id/reviews/:rid`
///
/// Returns a single review including the AI sentiment score and reply draft
/// (both `null` if not yet processed).
#[tracing::instrument(skip(state), name = "handlers::ai::get_review")]
pub async fn get_review(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((restaurant_id, review_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    let review = state
        .ai_service
        .get_review(
            &identity,
            RestaurantId::from_uuid(restaurant_id),
            ReviewId::from_uuid(review_id),
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(ReviewResponse::from(review)))
}

/// `POST /api/v1/restaurants/:id/reviews/analyse`
///
/// Runs AI sentiment analysis on all reviews in this restaurant that do not
/// yet have a `sentiment_score`.  Processes up to 50 reviews per call.
///
/// Returns immediately with `analysed: 0` when the `OpenAI` API key is not
/// configured (no error).
#[tracing::instrument(skip(state), name = "handlers::ai::analyse")]
pub async fn analyse_reviews(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let result = state
        .ai_service
        .analyse_pending_reviews(&identity, RestaurantId::from_uuid(restaurant_id))
        .await
        .map_err(ApiError::from)?;

    Ok(Json(AnalysisResponse {
        analysed: result.analysed,
        skipped: result.skipped,
        tokens_used: result.tokens_used,
    }))
}

/// `POST /api/v1/restaurants/:id/reviews/:rid/reply-draft`
///
/// Generates an AI reply draft for the review and saves it to the review
/// record.  Returns the draft text.
///
/// Returns `503 Service Unavailable` when the `OpenAI` API key is not set.
#[tracing::instrument(skip(state), name = "handlers::ai::reply_draft")]
pub async fn reply_draft(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((restaurant_id, review_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    let draft = state
        .ai_service
        .generate_reply_draft(
            &identity,
            RestaurantId::from_uuid(restaurant_id),
            ReviewId::from_uuid(review_id),
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(ReplyDraftResponse {
        review_id: review_id.to_string(),
        draft,
    }))
}

/// `POST /api/v1/restaurants/:id/reviews/:rid/reply-publish`
///
/// **Not yet implemented** — publishing replies requires OAuth tokens for
/// each review platform (Google My Business, Yelp Business Owner) which are
/// obtained through separate partner application flows.
///
/// Returns `501 Not Implemented` with guidance.
#[tracing::instrument(skip(state, identity), name = "handlers::ai::reply_publish")]
pub async fn reply_publish(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((restaurant_id, review_id)): Path<(Uuid, Uuid)>,
) -> impl IntoResponse {
    // Suppress "unused" warnings — these args are intentionally accepted so
    // the endpoint signature is future-proof when publishing is implemented.
    let _ = (&state, &identity, &restaurant_id, &review_id);
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "Reply publishing is not yet implemented.",
            "detail": "Publishing replies to Google and Yelp requires per-platform OAuth tokens obtained through their partner programmes.  This endpoint will be activated in a future release."
        })),
    )
}

// ---------------------------------------------------------------------------
// Token usage
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct _Empty {}

#[derive(Serialize)]
pub struct TokenUsageResponse {
    pub monthly_tokens_used: u64,
}

/// `GET /api/v1/ai/usage`
///
/// Returns the number of `OpenAI` tokens used by this tenant in the current
/// calendar month.
#[tracing::instrument(skip(state), name = "handlers::ai::token_usage")]
pub async fn token_usage(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
) -> ApiResult<impl IntoResponse> {
    let used = state
        .ai_service
        .get_token_usage(&identity)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(TokenUsageResponse {
        monthly_tokens_used: used,
    }))
}
