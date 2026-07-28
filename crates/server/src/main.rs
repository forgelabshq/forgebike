//! Binary entry point.
//!
//! Responsibilities (and nothing more):
//!
//! 1. Load the `.env` file (development only)
//! 2. Load and validate the [`Config`]
//! 3. Initialise the tracing subscriber
//! 4. Connect to `PostgreSQL` and Redis
//! 5. Run pending database migrations
//! 6. Build the [`AppState`] and the axum [`Router`]
//! 7. Bind the TCP listener and start serving
//!
//! Business logic, routing, and middleware all live in `forgebike-api`.

use std::sync::Arc;

use anyhow::Context;
use deadpool_redis::{Config as RedisPoolConfig, Runtime as RedisRuntime};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use forgebike_api::AppState;
use forgebike_config::{Config, Environment};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // -------------------------------------------------------------------------
    // 1. Load .env (silently ignored in production where vars come from the
    //    platform environment).
    // -------------------------------------------------------------------------
    dotenvy::dotenv().ok();

    // -------------------------------------------------------------------------
    // 2. Load configuration.
    // -------------------------------------------------------------------------
    let config = Config::load().context("Failed to load configuration")?;

    // -------------------------------------------------------------------------
    // 3. Initialise the tracing subscriber.
    //    Development → human-readable pretty output
    //    Production  → structured JSON (consumed by log-aggregation platforms)
    // -------------------------------------------------------------------------
    init_tracing(&config);

    tracing::info!(
        environment = %config.app.environment,
        version = env!("CARGO_PKG_VERSION"),
        "Starting Forgebike API server"
    );

    // -------------------------------------------------------------------------
    // 4a. PostgreSQL connection pool.
    // -------------------------------------------------------------------------
    let db = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await
        .context("Failed to connect to PostgreSQL")?;

    tracing::info!("PostgreSQL pool established");

    // -------------------------------------------------------------------------
    // 4b. Run pending migrations at startup so the schema is always up-to-date.
    //     Migrations are embedded at compile-time from the workspace root.
    // -------------------------------------------------------------------------
    sqlx::migrate!("../../migrations")
        .run(&db)
        .await
        .context("Failed to run database migrations")?;

    tracing::info!("Database migrations applied");

    // -------------------------------------------------------------------------
    // 4c. Redis connection pool.
    // -------------------------------------------------------------------------
    let redis = RedisPoolConfig::from_url(&config.redis.url)
        .create_pool(Some(RedisRuntime::Tokio1))
        .context("Failed to create Redis connection pool")?;

    // Verify we can reach Redis immediately so a misconfiguration fails fast.
    redis.get().await.context("Failed to connect to Redis")?;

    tracing::info!("Redis pool established");

    // -------------------------------------------------------------------------
    // 5. Assemble shared state and build the router.
    // -------------------------------------------------------------------------
    let state = AppState {
        db,
        redis,
        config: Arc::new(config.clone()),
    };

    let app = forgebike_api::router::build(state);

    // -------------------------------------------------------------------------
    // 6. Bind and serve.
    // -------------------------------------------------------------------------
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {addr}"))?;

    tracing::info!(%addr, "Server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("Server error")?;

    tracing::info!("Server shut down cleanly");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tracing initialisation
// ---------------------------------------------------------------------------

fn init_tracing(config: &Config) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.app.log_level));

    match config.app.environment {
        Environment::Production => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().json())
                .init();
        }
        Environment::Development => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().pretty())
                .init();
        }
    }
}

// ---------------------------------------------------------------------------
// Graceful shutdown — handles both Ctrl-C and SIGTERM (Docker / Kubernetes).
// ---------------------------------------------------------------------------

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c    => tracing::info!("Ctrl-C received"),
        () = terminate => tracing::info!("SIGTERM received"),
    }
}
