//! HTTP API layer.
//!
//! - `state`      — [`AppState`]: the shared handle cloned into every handler
//! - `router`     — assembles the axum [`Router`] with all middleware
//! - `handlers`   — one sub-module per feature area
//! - `middleware` — tower middleware functions
//! - `extractors` — custom axum extractors (role guards, validated JSON)
//! - `error`      — [`ApiError`] / [`ApiResult`]

pub mod error;
pub mod extractors;
pub mod handlers;
pub mod middleware;
pub mod router;
pub mod state;

pub use state::AppState;
