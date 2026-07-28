//! Assembles the complete axum [`Router`].
//!
//! This file is a *wiring manifest* — keep it thin.  No business logic,
//! no SQL, no token parsing.

use std::sync::Arc;

use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};
use tower_http::{
    cors::CorsLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

use crate::{handlers, middleware::auth::require_auth, state::AppState};

pub fn build(state: AppState) -> Router {
    // -----------------------------------------------------------------------
    // Shared middleware layers
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // Rate limiting — 5 req burst, 1 req/s steady state per IP.
    // Applied only to auth routes to throttle brute-force attempts.
    // -----------------------------------------------------------------------
    let rl = &state.config.rate_limit;
    let auth_governor = Arc::new(
        GovernorConfigBuilder::default()
            .per_second(rl.per_second)
            .burst_size(rl.burst_size)
            .finish()
            .unwrap(),
    );

    // -----------------------------------------------------------------------
    // Auth routes — rate-limited, no session required
    // -----------------------------------------------------------------------
    let auth_public = Router::new()
        .route("/register", post(handlers::auth::register))
        .route("/login", post(handlers::auth::login))
        .route("/refresh", post(handlers::auth::refresh))
        .route("/logout", post(handlers::auth::logout))
        .layer(GovernorLayer {
            config: auth_governor,
        });

    // Protected auth routes — session required
    let auth_protected = Router::new()
        .route("/me", get(handlers::auth::me))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

    // -----------------------------------------------------------------------
    // Full route tree
    // -----------------------------------------------------------------------
    Router::new()
        .route("/health", get(handlers::health::health))
        .nest("/api/v1/auth", auth_public.merge(auth_protected))
        // Future feature routers mount here:
        //   .nest("/api/v1/restaurants", restaurants::router(state.clone()))
        .layer(trace_layer)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
