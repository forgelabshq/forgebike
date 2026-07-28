//! Infrastructure layer — concrete adapters for every domain port.
//!
//! - `db` — `sqlx`-backed repository implementations
//! - `redis` — `deadpool_redis`-backed token store

pub mod db;
pub mod redis;
