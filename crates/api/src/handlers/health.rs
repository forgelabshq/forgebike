//! Health-check handler.
//!
//! `GET /health` returns a JSON body describing the liveness of each
//! infrastructure dependency.  The overall HTTP status is:
//!
//! - `200 OK`                  — all components healthy
//! - `503 Service Unavailable` — one or more components degraded

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::json;

use crate::state::AppState;

/// Checks the database and Redis connections and returns a structured health
/// report.
///
/// This endpoint is intentionally unauthenticated so that load-balancers and
/// orchestrators (Kubernetes, fly.io, Railway …) can probe it without
/// needing a token.
#[tracing::instrument(skip(state), name = "handlers::health")]
pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    // --- PostgreSQL ----------------------------------------------------------
    let db_ok = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();

    // --- Redis ---------------------------------------------------------------
    let redis_ok = state.redis.get().await.is_ok();

    // --- Aggregate -----------------------------------------------------------
    let all_ok = db_ok && redis_ok;

    let status = if all_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    if !all_ok {
        tracing::warn!(db_ok, redis_ok, "Health check degraded");
    }

    (
        status,
        Json(json!({
            "status": if all_ok { "ok" } else { "degraded" },
            "components": {
                "database": component_status(db_ok),
                "redis":    component_status(redis_ok),
            }
        })),
    )
}

#[inline]
fn component_status(ok: bool) -> &'static str {
    if ok { "ok" } else { "error" }
}
