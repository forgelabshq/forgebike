//! HTTP API layer.
//!
//! Responsibilities of this crate:
//!
//! - Build the [`axum::Router`] and wire up middleware
//! - Define request/response DTOs (serialisation only — no business logic)
//! - Map [`forgebike_domain::DomainError`] variants to HTTP status codes
//! - Hold [`AppState`] (the shared handle injected into every handler)
//!
//! This crate knows about HTTP but must **not** contain any business logic;
//! that belongs in `forgebike-application` (added in Phase 1+).

pub mod error;
pub mod handlers;
pub mod router;
pub mod state;

pub use state::AppState;
