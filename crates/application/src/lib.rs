//! Application layer — use-case orchestration.
//!
//! Services in this crate call domain ports (traits) and coordinate the steps
//! that make up each use case.  They know about domain types and configuration
//! but have **no** knowledge of HTTP, SQL, or Redis.

pub mod auth;
