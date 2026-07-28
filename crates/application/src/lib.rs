//! Application layer — use-case orchestration.
//!
//! Services call domain port traits and coordinate the steps that make up
//! each use case.  They know about domain types and configuration but have
//! **no** knowledge of HTTP, SQL, or Redis.

pub mod auth;
pub mod restaurant;
