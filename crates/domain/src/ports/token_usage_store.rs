//! Port trait for per-tenant AI token usage tracking.
//!
//! Usage is recorded in Redis as a monthly counter keyed by tenant ID.
//! Phase 8 (billing) will use these counters to enforce plan limits.

use async_trait::async_trait;

use crate::{identifiers::TenantId, DomainError};

#[async_trait]
pub trait TokenUsageStore: Send + Sync {
    /// Increment this tenant's token counter for the current calendar month
    /// and return the updated total.
    async fn record_usage(&self, tenant_id: TenantId, tokens_used: u64)
        -> Result<u64, DomainError>;

    /// Return the total tokens used by this tenant in the current calendar month.
    async fn get_monthly_usage(&self, tenant_id: TenantId) -> Result<u64, DomainError>;
}
