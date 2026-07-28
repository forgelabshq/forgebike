//! Infrastructure layer — concrete adapters for every domain port.
//!
//! - `db`             — `sqlx`-backed repository implementations
//! - `redis`          — `deadpool_redis`-backed token store
//! - `review_clients` — external review platform HTTP clients

pub mod db;
pub mod redis;
pub mod review_clients;
