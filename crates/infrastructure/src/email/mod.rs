//! SMTP email client using `lettre 0.11`.
//!
//! When `smtp_host` is empty the client is disabled gracefully — all
//! `send_email` calls return `Err(DomainError::ExternalService)` so callers
//! can surface a meaningful error rather than silently dropping messages.

use async_trait::async_trait;
use lettre::{
    message::Mailbox, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};

use forgebike_config::EmailConfig;
use forgebike_domain::{ports::email_port::EmailPort, DomainError};

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// SMTP email client backed by `lettre`.
///
/// A new SMTP transport is created per `send_email` call — connection pooling
/// is out of scope for Phase 7 but can be added later by hoisting the
/// transport into an `Arc<AsyncSmtpTransport<Tokio1Executor>>`.
pub struct LettreEmailClient {
    config: EmailConfig,
}

impl LettreEmailClient {
    #[must_use]
    pub fn new(config: &EmailConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Port implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl EmailPort for LettreEmailClient {
    fn is_configured(&self) -> bool {
        !self.config.smtp_host.is_empty()
    }

    async fn send_email(
        &self,
        to_address: &str,
        to_name: Option<&str>,
        subject: &str,
        body: &str,
    ) -> Result<(), DomainError> {
        if !self.is_configured() {
            return Err(DomainError::ExternalService(
                "Email not configured — set APP__EMAIL__SMTP_HOST to enable campaign delivery"
                    .into(),
            ));
        }

        // Build the `From` mailbox.
        let from_str = format!("{} <{}>", self.config.from_name, self.config.from_address);
        let from: Mailbox = from_str
            .parse()
            .map_err(|e: lettre::address::AddressError| DomainError::Internal(e.to_string()))?;

        // Build the `To` mailbox, including the display name when present.
        let to_str = match to_name {
            Some(n) if !n.is_empty() => format!("{n} <{to_address}>"),
            _ => to_address.to_string(),
        };
        let to: Mailbox = to_str
            .parse()
            .map_err(|e: lettre::address::AddressError| DomainError::Internal(e.to_string()))?;

        let message = Message::builder()
            .from(from)
            .to(to)
            .subject(subject)
            .body(body.to_string())
            .map_err(|e| DomainError::ExternalService(e.to_string()))?;

        let creds = Credentials::new(
            self.config.smtp_username.clone(),
            self.config.smtp_password.clone(),
        );

        let transport = AsyncSmtpTransport::<Tokio1Executor>::relay(&self.config.smtp_host)
            .map_err(|e| DomainError::ExternalService(format!("SMTP relay error: {e}")))?
            .port(self.config.smtp_port)
            .credentials(creds)
            .build();

        transport
            .send(message)
            .await
            .map_err(|e| DomainError::ExternalService(format!("SMTP send error: {e}")))?;

        Ok(())
    }
}
