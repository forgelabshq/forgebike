//! Assembles the top-level [`axum::Router`] with all middleware applied.
//!
//! Every feature area will add its own sub-router here as new phases are
//! implemented. Keep this file thin — it is a wiring manifest, not a place
//! for business logic.

use axum::{Router, routing::get};
use tower_http::{
    cors::CorsLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

use crate::{handlers, state::AppState};

/// Build the complete axum application.
///
/// Layers are applied bottom-up: the innermost layer runs first on the
/// request path and last on the response path.
pub fn build(state: AppState) -> Router {
    // ---------------------------------------------------------------------------
    // Tracing layer — logs every request/response pair as a structured span.
    // ---------------------------------------------------------------------------
    let trace_layer = TraceLayer::new_for_http()
        .make_span_with(
            DefaultMakeSpan::new()
                .level(Level::INFO)
                .include_headers(false),
        )
        .on_response(
            DefaultOnResponse::new()
                .level(Level::INFO)
                .include_headers(false),
        );

    // ---------------------------------------------------------------------------
    // CORS — permissive during development; tighten per-origin in production.
    // ---------------------------------------------------------------------------
    let cors_layer = CorsLayer::permissive();

    // ---------------------------------------------------------------------------
    // Route tree
    // ---------------------------------------------------------------------------
    Router::new()
        // Infrastructure / ops
        .route("/health", get(handlers::health::health))
        // Future feature sub-routers mount here, e.g.:
        //   .nest("/api/v1/auth",        auth::router())
        //   .nest("/api/v1/restaurants", restaurants::router())
        .layer(trace_layer)
        .layer(cors_layer)
        .with_state(state)
}
