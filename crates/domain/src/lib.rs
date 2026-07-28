//! Domain layer — pure business logic with zero infrastructure dependencies.
//!
//! Contains:
//! - **Entities** — rich types that carry business rules
//! - **Identifiers** — newtype UUID wrappers that prevent ID mix-ups
//! - **Pagination** — cursor-based pagination primitives
//! - **Ports** — `async_trait` traits that infrastructure must implement
//! - **Errors** — the single `DomainError` type returned by every port

pub mod entities;
pub mod error;
pub mod identifiers;
pub mod pagination;
pub mod ports;

pub use error::DomainError;
