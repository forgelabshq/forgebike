//! Shared application state — injected into every axum handler via
//! [`axum::extract::State`].

use std::sync::Arc;

use deadpool_redis::Pool as RedisPool;
use forgebike_application::auth::AuthService;
use forgebike_config::Config;
use sqlx::PgPool;

/// Shared, cheaply-cloned state available to every HTTP handler.
///
/// All fields are either `Clone` themselves (`PgPool`, `RedisPool`) or wrapped
/// in `Arc` so cloning is O(1).
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
}
