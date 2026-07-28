//! Shared application state injected into every axum handler via
//! [`axum::extract::State`].
//!
//! [`AppState`] is intentionally `Clone` (all inner fields are either `Clone`
//! or wrapped in `Arc`) so that axum can cheaply hand a copy to each handler
//! future without any locking.

use deadpool_redis::Pool as RedisPool;
use forgebike_config::Config;
use sqlx::PgPool;
use std::sync::Arc;

/// Shared state available to every HTTP handler.
#[derive(Clone)]
pub struct AppState {
    /// `PostgreSQL` connection pool.
    pub db: PgPool,

    /// Redis connection pool (cache, rate-limit counters, job queues).
    pub redis: RedisPool,

    /// Validated application configuration (read-only after startup).
    pub config: Arc<Config>,
}
