//! Redis-backed implementation of [`TokenUsageStore`].
//!
//! ## Key format
//! `ai:tokens:{tenant_id_hex}:{YYYYMM}`
//!
//! ## TTL
//! Each key is given a 62-day TTL on first write, covering the remainder of
//! the current month plus the full following month.  The TTL is refreshed on
//! every increment so the key never expires while it is actively used.

use async_trait::async_trait;
use chrono::Utc;
use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool as RedisPool;

use forgebike_domain::{
    identifiers::TenantId, ports::token_usage_store::TokenUsageStore, DomainError,
};

/// 62 days in seconds — covers current + following month with a small buffer.
const TTL_SECS: u64 = 62 * 24 * 60 * 60;

pub struct RedisTokenUsageStore {
    pool: RedisPool,
}

impl RedisTokenUsageStore {
    #[must_use]
    pub fn new(pool: RedisPool) -> Self {
        Self { pool }
    }

    fn key(tenant_id: TenantId) -> String {
        let month = Utc::now().format("%Y%m");
        format!("ai:tokens:{tenant_id}:{month}")
    }
}

#[async_trait]
impl TokenUsageStore for RedisTokenUsageStore {
    async fn record_usage(
        &self,
        tenant_id: TenantId,
        tokens_used: u64,
    ) -> Result<u64, DomainError> {
        let key = Self::key(tenant_id);

        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        // INCRBY returns the new total.
        let new_total: u64 = conn
            .incr(&key, tokens_used)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        // Refresh TTL on every write.
        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        conn.expire::<_, ()>(&key, TTL_SECS as i64)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(new_total)
    }

    async fn get_monthly_usage(&self, tenant_id: TenantId) -> Result<u64, DomainError> {
        let key = Self::key(tenant_id);

        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        let val: Option<u64> = conn
            .get(&key)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(val.unwrap_or(0))
    }
}
