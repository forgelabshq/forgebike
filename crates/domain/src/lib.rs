//! Domain layer — pure business logic.
//!
//! This crate has **no** knowledge of HTTP, databases, or any external
//! service.  It contains only:
//!
//! - Entities and value objects
//! - Newtype ID wrappers
//! - Domain error types
//! - Port traits (interfaces that infrastructure must implement)
//!
//! Every other crate may depend on this one; this crate must depend on none
//! of the others.

pub mod error;
pub mod identifiers;

pub use error::DomainError;
