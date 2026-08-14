//! Binary entry point — the composition root.
//!
//! Responsibilities (and nothing more):
//!
//! 1. Load `.env`
//! 2. Load [`Config`]
//! 3. Initialise tracing
//! 4. Connect to `PostgreSQL` and Redis
//! 5. Run pending migrations
//! 6. Wire up repositories → services → [`AppState`]
//! 7. Build the router and serve

use std::{net::SocketAddr, sync::Arc};

use anyhow::Context;
use deadpool_redis::{Config as RedisPoolConfig, Runtime as RedisRuntime};
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use forgebike_api::AppState;
use forgebike_application::{
    ai::AiService, analytics::AnalyticsService, auth::AuthService, content::ContentService,
    restaurant::RestaurantService, review::ReviewService,
};
use forgebike_config::{Config, Environment};
use forgebike_infrastructure::{
    ai::OpenAiClient,
    db::{
        PgAnalyticsRepository, PgContentRepository, PgMenuItemRepository, PgRestaurantRepository,
        PgReviewRepository, PgTenantRepository, PgUserRepository,
    },
    redis::{RedisTokenStore, RedisTokenUsageStore},
    review_clients::{GooglePlacesClient, TripAdvisorClient, YelpFusionClient},
};

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    // -------------------------------------------------------------------------
    // 1. .env
    // -------------------------------------------------------------------------
    dotenvy::dotenv().ok();

    // -------------------------------------------------------------------------
    // 2. Config
    // -------------------------------------------------------------------------
    let config = Config::load().context("Failed to load configuration")?;

    // -------------------------------------------------------------------------
    // 3. Tracing
    // -------------------------------------------------------------------------
    init_tracing(&config);

    tracing::info!(
        environment = %config.app.environment,
        version = env!("CARGO_PKG_VERSION"),
        "Starting Forgebike API server"
    );

    // Warn loudly if the default JWT secret is in use.
    if config.jwt.secret == "change-me-in-production-use-openssl-rand-hex-32" {
        tracing::warn!("Using the default JWT secret — set APP__JWT__SECRET in production!");
    }

    // -------------------------------------------------------------------------
    // 4a. PostgreSQL
    // -------------------------------------------------------------------------
    let db = PgPoolOptions::new()
        .max_connections(config.database.max_connections)
        .connect(&config.database.url)
        .await
        .context("Failed to connect to PostgreSQL")?;

    tracing::info!("PostgreSQL pool established");

    // -------------------------------------------------------------------------
    // 4b. Migrations
    // -------------------------------------------------------------------------
    sqlx::migrate!("../../migrations")
        .run(&db)
        .await
        .context("Failed to run database migrations")?;

    tracing::info!("Database migrations applied");

    // -------------------------------------------------------------------------
    // 4c. Redis
    // -------------------------------------------------------------------------
    let redis = RedisPoolConfig::from_url(&config.redis.url)
        .create_pool(Some(RedisRuntime::Tokio1))
        .context("Failed to create Redis connection pool")?;

    redis.get().await.context("Failed to connect to Redis")?;

    tracing::info!("Redis pool established");

    // -------------------------------------------------------------------------
    // 5. Wire up infrastructure → application
    // -------------------------------------------------------------------------
    let user_repo = Arc::new(PgUserRepository::new(db.clone()));
    let tenant_repo = Arc::new(PgTenantRepository::new(db.clone()));
    let token_store = Arc::new(RedisTokenStore::new(redis.clone()));

    let auth_service = Arc::new(AuthService::new(
        user_repo,
        tenant_repo,
        token_store,
        config.jwt.clone(),
    ));

    let restaurant_repo = Arc::new(PgRestaurantRepository::new(db.clone()));
    let menu_item_repo = Arc::new(PgMenuItemRepository::new(db.clone()));
    let restaurant_service = Arc::new(RestaurantService::new(
        Arc::clone(&restaurant_repo) as _,
        Arc::clone(&menu_item_repo) as _,
    ));

    let review_repo = Arc::new(PgReviewRepository::new(db.clone()));
    let review_service = Arc::new(ReviewService::new(
        review_repo as _,
        Arc::clone(&restaurant_repo) as _,
        Arc::new(GooglePlacesClient::new(
            &config.external_apis.google_places_api_key,
        )) as _,
        Arc::new(YelpFusionClient::new(&config.external_apis.yelp_api_key)) as _,
        Arc::new(TripAdvisorClient::new(
            &config.external_apis.tripadvisor_api_key,
        )) as _,
    ));

    let review_repo_for_ai = Arc::new(PgReviewRepository::new(db.clone()));
    let ai_client = Arc::new(OpenAiClient::new(
        &config.ai.openai_api_key,
        &config.ai.model,
        config.ai.max_sentiment_tokens,
        config.ai.max_reply_tokens,
        config.ai.max_content_tokens,
    ));
    let token_usage = Arc::new(RedisTokenUsageStore::new(redis.clone()));
    let ai_service = Arc::new(AiService::new(
        review_repo_for_ai as _,
        Arc::clone(&restaurant_repo) as _,
        Arc::clone(&ai_client) as _,
        Arc::clone(&token_usage) as _,
    ));

    let content_repo = Arc::new(PgContentRepository::new(db.clone()));
    let content_service = Arc::new(ContentService::new(
        content_repo as _,
        Arc::clone(&restaurant_repo) as _,
        Arc::clone(&ai_client) as _,
        Arc::clone(&token_usage) as _,
    ));

    let analytics_repo = Arc::new(PgAnalyticsRepository::new(db.clone()));
    let analytics_service = Arc::new(AnalyticsService::new(
        analytics_repo as _,
        Arc::clone(&restaurant_repo) as _,
    ));

    tracing::info!("Services wired");

    // -------------------------------------------------------------------------
    // 6. AppState + router
    // -------------------------------------------------------------------------
    let state = AppState {
        db,
        redis,
        config: Arc::new(config.clone()),
        auth_service,
        restaurant_service,
        review_service,
        ai_service,
        content_service,
        analytics_service,
    };

    let app = forgebike_api::router::build(state);

    // -------------------------------------------------------------------------
    // 7. Serve
    // -------------------------------------------------------------------------
    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {addr}"))?;

    tracing::info!(%addr, "Server listening");

    axum::serve(
        listener,
        // into_make_service_with_connect_info populates ConnectInfo<SocketAddr>
        // in each request, which tower_governor needs to key rate limits by IP.
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("Server error")?;

    tracing::info!("Server shut down cleanly");
    Ok(())
}

// ---------------------------------------------------------------------------
// Tracing
// ---------------------------------------------------------------------------

fn init_tracing(config: &Config) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&config.app.log_level));

    match config.app.environment {
        Environment::Production => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json())
                .init();
        }
        Environment::Development => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().pretty())
                .init();
        }
    }
}

// ---------------------------------------------------------------------------
// Graceful shutdown
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
