//! Port trait for payment-provider (Stripe) integration.
//!
//! The concrete adapter lives in `forgebike_infrastructure::stripe`.
//! Keeping signature verification behind a trait means the application layer
//! never imports the `hmac` / `sha2` crates directly and the logic is easily
//! testable with a no-op mock.

use crate::DomainError;

/// Abstraction over the Stripe webhook verification protocol.
#[async_trait::async_trait]
pub trait BillingPort: Send + Sync {
    /// Verify a Stripe webhook signature header against the raw request body.
    ///
    /// Stripe attaches a `Stripe-Signature` header of the form
    /// `t=<timestamp>,v1=<hmac>`.  This method checks that the HMAC
    /// matches and that the timestamp is within a 5-minute tolerance.
    ///
    /// Returns `Ok(())` on success, or `Err(DomainError::Validation)` if
    /// the signature is invalid or the payload is too old.
    fn verify_webhook_signature(
        &self,
        payload: &[u8],
        stripe_signature: &str,
    ) -> Result<(), DomainError>;
}
