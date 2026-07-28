//! Port traits — the interfaces that infrastructure adapters must implement.
//!
//! These are the "hexagon's edges": the domain defines what it needs;
//! `forgebike-infrastructure` provides the concrete implementations.

pub mod tenant_repository;
pub mod token_store;
pub mod user_repository;
