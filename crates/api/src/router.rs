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

use crate::{
    handlers,
    middleware::auth::{require_auth, require_ws_auth},
    state::AppState,
};

fn restaurant_routes(state: &AppState) -> Router<AppState> {
    use axum::routing::patch;

    Router::new()
        // Restaurant CRUD
        .route(
            "/",
            post(handlers::restaurants::create_restaurant)
                .get(handlers::restaurants::list_restaurants),
        )
        .route(
            "/:id",
            get(handlers::restaurants::get_restaurant)
                .patch(handlers::restaurants::update_restaurant)
                .delete(handlers::restaurants::delete_restaurant),
        )
        // Menu items
        .route(
            "/:id/menu",
            post(handlers::restaurants::create_menu_item)
                .get(handlers::restaurants::list_menu_items),
        )
        .route(
            "/:id/menu/:item_id",
            patch(handlers::restaurants::update_menu_item)
                .delete(handlers::restaurants::delete_menu_item),
        )
        // Reviews
        .route("/:id/reviews", get(handlers::reviews::list_reviews))
        .route("/:id/reviews/sync", post(handlers::reviews::sync_reviews))
        // AI features
        .route("/:id/reviews/analyse", post(handlers::ai::analyse_reviews))
        .route("/:id/reviews/:rid", get(handlers::ai::get_review))
        .route(
            "/:id/reviews/:rid/reply-draft",
            post(handlers::ai::reply_draft),
        )
        .route(
            "/:id/reviews/:rid/reply-publish",
            post(handlers::ai::reply_publish),
        )
        // Content (AI-generated marketing pieces)
        .route("/:id/content/generate", post(handlers::content::generate))
        .route("/:id/content/stream", get(handlers::content::stream))
        .route("/:id/content", get(handlers::content::list))
        .route(
            "/:id/content/:cid",
            get(handlers::content::get_piece)
                .patch(handlers::content::update)
                .delete(handlers::content::delete),
        )
        // Analytics (BI dashboard endpoints)
        .route(
            "/:id/analytics/overview",
            get(handlers::analytics::overview),
        )
        .route(
            "/:id/analytics/reviews",
            get(handlers::analytics::reviews_analytics),
        )
        .route(
            "/:id/analytics/content",
            get(handlers::analytics::content_analytics),
        )
        // Customer contacts
        .route(
            "/:id/contacts",
            post(handlers::contacts::create_contact).get(handlers::contacts::list_contacts),
        )
        .route(
            "/:id/contacts/import",
            post(handlers::contacts::import_contacts),
        )
        .route(
            "/:id/contacts/:cid",
            get(handlers::contacts::get_contact)
                .patch(handlers::contacts::update_contact)
                .delete(handlers::contacts::delete_contact),
        )
        // Campaigns
        .route(
            "/:id/campaigns",
            post(handlers::campaigns::create_campaign).get(handlers::campaigns::list_campaigns),
        )
        .route(
            "/:id/campaigns/:cid",
            get(handlers::campaigns::get_campaign)
                .patch(handlers::campaigns::update_campaign)
                .delete(handlers::campaigns::delete_campaign),
        )
        .route(
            "/:id/campaigns/:cid/send",
            post(handlers::campaigns::send_campaign),
        )
        // All restaurant routes require authentication.
        .layer(middleware::from_fn_with_state(state.clone(), require_auth))
}

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
    // AI usage — requires auth, scoped to tenant (no restaurant ID)
    let ai_routes = Router::new()
        .route("/usage", get(handlers::ai::token_usage))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .route("/health", get(handlers::health::health))
        // WebSocket chat — require_ws_auth middleware runs BEFORE the handler so
        // that auth is checked before WebSocketUpgrade extraction is attempted.
        // Plain HTTP GETs without a valid ?token= get 401; plain GETs with a
        // valid token get 400/426 (Upgrade Required) from WebSocketUpgrade.
        .route(
            "/api/v1/ws/chat/:restaurant_id",
            get(handlers::chat::chat_ws).layer(middleware::from_fn_with_state(
                state.clone(),
                require_ws_auth,
            )),
        )
        .nest("/api/v1/auth", auth_public.merge(auth_protected))
        .nest("/api/v1/restaurants", restaurant_routes(&state))
        .nest("/api/v1/ai", ai_routes)
        .layer(trace_layer)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
