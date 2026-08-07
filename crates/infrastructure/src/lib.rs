//! Infrastructure layer — concrete adapters for every domain port.
//!
//! - `ai`             — `OpenAI` client implementing `AiContentPort`
//! - `db`             — `sqlx`-backed repository implementations
//! - `redis`          — `deadpool_redis`-backed token and usage stores
//! - `review_clients` — external review platform HTTP clients

pub mod ai;
pub mod db;
pub mod redis;
pub mod review_clients;
