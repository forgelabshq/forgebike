//! Application configuration.
//!
//! Config is loaded in layers, with each subsequent layer overriding the previous:
//!
//! 1. `config/default.toml`  — committed baseline values (no secrets)
//! 2. `config/{APP_ENV}.toml` — optional environment-specific overrides
//! 3. Environment variables   — `APP__` prefix, `__` separator
//!    e.g. `APP__DATABASE__URL` → `database.url`
//!
//! In addition, the bare `DATABASE_URL` environment variable is accepted as an
//! alias for `APP__DATABASE__URL` to stay compatible with common tooling (sqlx
//! CLI, Railway, Fly.io, etc.).

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub redis: RedisConfig,
    pub app: AppConfig,
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
            Environment::Development => write!(f, "development"),
            Environment::Production => write!(f, "production"),
        }
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

impl Config {
    /// Load the configuration from files and environment variables.
    ///
    /// # Errors
    /// Returns a [`config::ConfigError`] if any required key is missing or a
    /// value cannot be deserialised into the expected type.
    pub fn load() -> Result<Self, config::ConfigError> {
        // Honour the bare `DATABASE_URL` convention before handing off to the
        // layered loader.  We do this by mapping it into the `APP__` namespace
        // so the rest of the loading logic stays uniform.
        if let Ok(url) = std::env::var("DATABASE_URL") {
            // Only set if the app-namespaced override isn't already present.
            if std::env::var("APP__DATABASE__URL").is_err() {
                // SAFETY: setting env vars in a single-threaded startup context.
                std::env::set_var("APP__DATABASE__URL", url);
            }
        }

        let env = std::env::var("APP_ENV").unwrap_or_else(|_| "development".into());

        let cfg = config::Config::builder()
            // 1. Committed defaults (no secrets).
            .add_source(config::File::with_name("config/default"))
            // 2. Optional per-environment overrides.
            .add_source(config::File::with_name(&format!("config/{env}")).required(false))
            // 3. Environment variable overrides — APP__SECTION__KEY=value.
            .add_source(
                config::Environment::with_prefix("APP")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?;

        cfg.try_deserialize()
    }
}
