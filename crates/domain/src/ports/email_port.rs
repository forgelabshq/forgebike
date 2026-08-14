//! Port trait for outbound email delivery.
//!
//! The concrete implementation (`LettreEmailClient`) lives in
//! `forgebike_infrastructure::email`.  Tests use a simple in-memory mock.

use async_trait::async_trait;

use crate::DomainError;

// ---------------------------------------------------------------------------
// Port
// ---------------------------------------------------------------------------

#[async_trait]
pub trait EmailPort: Send + Sync {
    /// Send a plain-text email.
    ///
    /// Returns `Err(DomainError::ExternalService)` when the SMTP client is
    /// not configured (empty `smtp_host`) or the send fails.
    async fn send_email(
        &self,
        to_address: &str,
        to_name: Option<&str>,
        subject: &str,
        body: &str,
    ) -> Result<(), DomainError>;

    /// `true` when the SMTP host is configured and emails can be sent.
    fn is_configured(&self) -> bool;
}
