//! Stripe billing webhook handler.
//!
//! ## Endpoint
//! `POST /api/v1/billing/webhook`
//!
//! This endpoint receives raw Stripe webhook events.  Authentication is via
//! the `Stripe-Signature` header (HMAC-SHA256 over the raw body), **not** a
//! Bearer token — Stripe cannot set custom auth headers.
//!
//! ## Security
//! - The raw `Bytes` body is used for signature verification before any JSON
//!   parsing occurs.
//! - When `APP__STRIPE__WEBHOOK_SECRET` is empty (development / CI) the
//!   signature check is bypassed so integration tests work without a live
//!   Stripe account.
//! - Always returns `200 OK` on success so Stripe does not retry the event.
//!   Returns `400` on signature failure and `422` on parse errors.

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde_json::json;

use crate::{error::ApiResult, state::AppState};

/// `POST /api/v1/billing/webhook`
///
/// Processes Stripe subscription lifecycle events:
/// - `customer.subscription.created`  → upgrades tenant plan
/// - `customer.subscription.updated`  → changes tenant plan
/// - `customer.subscription.deleted`  → downgrades to Starter
///
/// Other event types are acknowledged with `200` and ignored.
#[tracing::instrument(skip(state, body), name = "handlers::billing::webhook")]
pub async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<impl IntoResponse> {
    let signature = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    state
        .billing_service
        .handle_stripe_webhook(&body, signature)
        .await
        .map_err(crate::error::ApiError::from)?;

    Ok((StatusCode::OK, Json(json!({ "received": true }))))
}
