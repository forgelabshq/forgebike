//! Application configuration.
//!
//! Config is loaded in layers, each layer overriding the previous:
//!
//! 1. `config/default.toml`        — committed baseline (no secrets)
//! 2. `config/{APP_ENV}.toml`      — optional per-environment overrides
//! 3. `APP__*` environment variables — `__` separator maps to nested keys
//!    e.g. `APP__DATABASE__URL` → `database.url`
//!
//! `DATABASE_URL` (bare, without prefix) is also accepted and re-mapped
//! into the `APP__DATABASE__URL` slot before the loader runs, keeping us
//! compatible with Heroku/Railway/Fly.io and the `sqlx` CLI.

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Top-level
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub app: AppConfig,
    pub jwt: JwtConfig,
    pub rate_limit: RateLimitConfig,
    pub external_apis: ExternalApisConfig,
    pub ai: AiConfig,
}

/// Configuration for the `OpenAI` integration.
#[derive(Debug, Deserialize, Clone)]
pub struct AiConfig {
    /// `OpenAI` API key.  Set via `APP__AI__OPENAI_API_KEY`.
    /// When empty the AI features are disabled gracefully.
    pub openai_api_key: String,
    /// Model to use for all AI calls.  Default: `gpt-4o-mini`.
    pub model: String,
    /// Maximum tokens for a sentiment analysis response.
    pub max_sentiment_tokens: u32,
    /// Maximum tokens for a reply draft response.
    pub max_reply_tokens: u32,
    /// Maximum tokens for marketing content generation.
    pub max_content_tokens: u32,
}

/// API keys for external review platforms.
/// All fields default to empty strings — the corresponding client skips
/// fetching when its key is empty rather than returning an error.
#[derive(Debug, Deserialize, Clone)]
pub struct ExternalApisConfig {
    pub google_places_api_key: String,
    pub yelp_api_key: String,
    pub tripadvisor_api_key: String,
}

// ---------------------------------------------------------------------------
// Sub-sections
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RedisConfig {
    pub url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub environment: Environment,
    pub log_level: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RateLimitConfig {
    /// Burst capacity — number of requests allowed in an instant before
    /// throttling kicks in. Raise this in development/testing.
    pub burst_size: u32,
    /// Steady-state replenishment rate: tokens added per second.
    pub per_second: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct JwtConfig {
    /// HS256 signing secret — must be at least 32 characters in production.
    /// Set via `APP__JWT__SECRET` environment variable; never commit the real
    /// value.
    pub secret: String,

    /// Lifetime of an access token in seconds. Default: 900 (15 minutes).
    pub access_token_expiry_secs: u64,

    /// Lifetime of a refresh token in seconds. Default: 604800 (7 days).
    pub refresh_token_expiry_secs: u64,
}

// ---------------------------------------------------------------------------
// Environment enum
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    Development,
    Production,
}

impl std::fmt::Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Development => write!(f, "development"),
            Self::Production => write!(f, "production"),
        }
    }
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

impl Config {
    /// Load configuration from files and environment variables.
    ///
    /// # Errors
    /// Returns [`config::ConfigError`] if a required key is absent or a value
    /// cannot be deserialised into the expected type.
    pub fn load() -> Result<Self, config::ConfigError> {
        // Allow bare DATABASE_URL (set by most PaaS platforms and sqlx-cli)
        // by mirroring it into the APP__ namespace before loading.
        if let Ok(url) = std::env::var("DATABASE_URL") {
            if std::env::var("APP__DATABASE__URL").is_err() {
                std::env::set_var("APP__DATABASE__URL", url);
            }
        }

        let env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".into());

        config::Config::builder()
            .add_source(config::File::with_name("config/default"))
            .add_source(config::File::with_name(&format!("config/{env}")).required(false))
            .add_source(
                config::Environment::with_prefix("APP")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?
            .try_deserialize()
    }
}
