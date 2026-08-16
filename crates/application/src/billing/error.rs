//! Error type for the billing application service.

use thiserror::Error;

use forgebike_domain::{identifiers::TenantId, DomainError};

#[derive(Debug, Error)]
pub enum BillingError {
    /// Stripe webhook signature was missing, invalid, or too old.
    #[error("Invalid webhook signature: {0}")]
    InvalidSignature(String),

    /// The Stripe event references a customer ID not in our system.
    #[error("Stripe customer {0} not found")]
    CustomerNotFound(String),

    /// Tenant was not found.
    #[error("Tenant {0} not found")]
    TenantNotFound(TenantId),

    /// The tenant has exceeded their monthly AI token budget for this plan.
    #[error("Monthly AI token budget exceeded: used {used}, limit {limit}")]
    BudgetExceeded { used: u64, limit: u64 },

    /// The requested operation is not available on the tenant's current plan.
    #[error("Feature not available on the {plan} plan")]
    FeatureNotAvailable { plan: String },

    /// Caller does not have permission (e.g. non-admin calling admin endpoint).
    #[error("Access denied")]
    Forbidden,

    /// The webhook payload could not be parsed as a Stripe event.
    #[error("Could not parse Stripe event: {0}")]
    ParseError(String),

    #[error(transparent)]
    Domain(#[from] DomainError),
}
