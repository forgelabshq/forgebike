//! Shared application state — injected into every axum handler via
//! [`axum::extract::State`].

use std::sync::Arc;

use deadpool_redis::Pool as RedisPool;
use forgebike_application::{
    ai::AiService, auth::AuthService, content::ContentService, restaurant::RestaurantService,
    review::ReviewService,
};
use forgebike_config::Config;
use sqlx::PgPool;

/// Shared, cheaply-cloned state available to every HTTP handler.
#[derive(Clone)]
pub struct AppState {
    /// `PostgreSQL` connection pool.
    pub db: PgPool,

    /// Redis connection pool.
    pub redis: RedisPool,

    /// Validated application configuration (read-only after startup).
    pub config: Arc<Config>,

    /// Authentication use-case service.
    pub auth_service: Arc<AuthService>,

    /// Restaurant and menu-item use-case service.
    pub restaurant_service: Arc<RestaurantService>,

    /// Review sync and listing use-case service.
    pub review_service: Arc<ReviewService>,

    /// AI sentiment analysis and reply draft use-case service.
    pub ai_service: Arc<AiService>,

    /// AI marketing content generation and management use-case service.
    pub content_service: Arc<ContentService>,
}
