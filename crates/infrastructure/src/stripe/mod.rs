//! Stripe webhook adapter implementing [`BillingPort`].
//!
//! Verifies the `Stripe-Signature` header using HMAC-SHA256 without
//! depending on the full Stripe Rust SDK.
//!
//! ## Signature format
//! `Stripe-Signature: t=<unix_ts>,v1=<hex_hmac>`
//!
//! ## Verification algorithm
//! 1. Parse `t` and `v1` from the header.
//! 2. Compute `HMAC-SHA256(secret, "<t>.<raw_body>")`.
//! 3. Compare computed digest with `v1` in constant time.
//! 4. Reject if |now − t| > 300 seconds (replay protection).

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use sha2::Sha256;

use forgebike_config::StripeConfig;
use forgebike_domain::{ports::billing_port::BillingPort, DomainError};

type HmacSha256 = Hmac<Sha256>;

/// Stripe webhook signature verifier.
///
/// When `webhook_secret` is empty, all signatures are accepted — this is
/// intentional for development / CI where no real Stripe keys are configured.
pub struct StripeClient {
    config: StripeConfig,
}

impl StripeClient {
    #[must_use]
    pub fn new(config: &StripeConfig) -> Self {
        Self {
            config: config.clone(),
        }
    }
}

impl BillingPort for StripeClient {
    fn verify_webhook_signature(
        &self,
        payload: &[u8],
        stripe_signature: &str,
    ) -> Result<(), DomainError> {
        // Dev / CI shortcut: accept all webhooks when no secret is configured.
        if self.config.webhook_secret.is_empty() {
            return Ok(());
        }

        // --- Parse the Stripe-Signature header ---
        let (timestamp, sig_hex) = parse_stripe_signature(stripe_signature)?;

        // --- Replay-attack protection (±5 minutes) ---
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if now.abs_diff(timestamp) > 300 {
            return Err(DomainError::Validation(
                "Stripe webhook timestamp is outside the 5-minute tolerance window".into(),
            ));
        }

        // --- Compute expected HMAC ---
        let signed_payload = format!("{timestamp}.{}", String::from_utf8_lossy(payload));
        let mut mac = HmacSha256::new_from_slice(self.config.webhook_secret.as_bytes())
            .map_err(|_| DomainError::Internal("Failed to create HMAC key".into()))?;
        mac.update(signed_payload.as_bytes());
        let expected = mac.finalize().into_bytes();

        // --- Decode the provided signature ---
        let provided = hex::decode(sig_hex)
            .map_err(|_| DomainError::Validation("Stripe-Signature v1 is not valid hex".into()))?;

        // --- Constant-time comparison ---
        if expected.as_slice() != provided.as_slice() {
            return Err(DomainError::Validation(
                "Stripe webhook signature mismatch".into(),
            ));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse `t=<ts>,v1=<sig>` from a Stripe-Signature header value.
/// Returns `(timestamp_secs, signature_hex)`.
fn parse_stripe_signature(header: &str) -> Result<(u64, &str), DomainError> {
    let mut timestamp: Option<u64> = None;
    let mut signature: Option<&str> = None;

    for part in header.split(',') {
        if let Some(v) = part.strip_prefix("t=") {
            timestamp = v.parse::<u64>().ok();
        } else if let Some(v) = part.strip_prefix("v1=") {
            signature = Some(v);
        }
    }

    let t = timestamp.ok_or_else(|| {
        DomainError::Validation("Stripe-Signature header missing 't' field".into())
    })?;
    let s = signature.ok_or_else(|| {
        DomainError::Validation("Stripe-Signature header missing 'v1' field".into())
    })?;

    Ok((t, s))
}
