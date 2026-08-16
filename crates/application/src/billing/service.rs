//! [`BillingService`] — subscription plan management and Stripe webhook processing.
//!
//! ## Responsibilities
//! - Verify and process incoming Stripe webhook events
//! - Expose the current plan + limits for any tenant
//! - Check whether a tenant is within their AI token budget
//! - Allow admins to set a tenant's plan tier directly
//! - Run a usage audit (called daily from a background task in the server)

use std::sync::Arc;

use forgebike_config::StripeConfig;
use forgebike_domain::{
    entities::tenant::{PlanLimits, PlanTier, Tenant},
    identifiers::TenantId,
    ports::{
        billing_port::BillingPort, tenant_repository::TenantRepository,
        token_usage_store::TokenUsageStore,
    },
};

use super::error::BillingError;

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

pub struct BillingService {
    tenants: Arc<dyn TenantRepository>,
    billing: Arc<dyn BillingPort>,
    token_usage: Arc<dyn TokenUsageStore>,
    stripe_config: StripeConfig,
}

impl BillingService {
    pub fn new(
        tenants: Arc<dyn TenantRepository>,
        billing: Arc<dyn BillingPort>,
        token_usage: Arc<dyn TokenUsageStore>,
        stripe_config: StripeConfig,
    ) -> Self {
        Self {
            tenants,
            billing,
            token_usage,
            stripe_config,
        }
    }

    // -----------------------------------------------------------------------
    // Webhook
    // -----------------------------------------------------------------------

    /// Verify and process an incoming Stripe webhook event.
    ///
    /// Subscription lifecycle events (`created`, `updated`, `deleted`) are
    /// mapped to plan tier changes in the tenant repository.  All other event
    /// types are acknowledged and ignored (idempotent).
    #[allow(clippy::too_many_lines)]
    pub async fn handle_stripe_webhook(
        &self,
        payload: &[u8],
        stripe_signature: &str,
    ) -> Result<(), BillingError> {
        // 1. Verify the Stripe HMAC signature.
        self.billing
            .verify_webhook_signature(payload, stripe_signature)
            .map_err(|e| BillingError::InvalidSignature(e.to_string()))?;

        // 2. Parse the raw JSON body.
        let event: serde_json::Value =
            serde_json::from_slice(payload).map_err(|e| BillingError::ParseError(e.to_string()))?;

        // 3. Dispatch on event type.
        let event_type = event["type"].as_str().unwrap_or("");

        match event_type {
            "customer.subscription.created"
            | "customer.subscription.updated"
            | "customer.subscription.deleted" => {
                let customer_id = event["data"]["object"]["customer"].as_str().unwrap_or("");

                // Map the event to the new plan tier.
                let new_plan = if event_type == "customer.subscription.deleted" {
                    PlanTier::Starter
                } else {
                    let price_id = event["data"]["object"]["items"]["data"][0]["price"]["id"]
                        .as_str()
                        .unwrap_or("");
                    if price_id == self.stripe_config.price_id_growth {
                        PlanTier::Growth
                    } else if price_id == self.stripe_config.price_id_scale {
                        PlanTier::Scale
                    } else {
                        // Unknown price — downgrade safely rather than crashing.
                        PlanTier::Starter
                    }
                };

                // Look up the tenant by their Stripe customer ID.
                let tenant = self
                    .tenants
                    .find_by_stripe_customer_id(customer_id)
                    .await?
                    .ok_or_else(|| BillingError::CustomerNotFound(customer_id.to_string()))?;

                // Persist the new plan.
                self.tenants
                    .update_plan(tenant.id, new_plan.clone(), Some(customer_id))
                    .await?;

                tracing::info!(tenant_id = %tenant.id, plan = %new_plan, "Stripe plan updated");
            }

            // All other Stripe event types are acknowledged without action.
            _ => {}
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Plan introspection
    // -----------------------------------------------------------------------

    /// Return the current plan tier and its associated resource limits for a
    /// given tenant.
    pub async fn get_plan(
        &self,
        tenant_id: TenantId,
    ) -> Result<(PlanTier, PlanLimits), BillingError> {
        let tenant = self
            .tenants
            .find_by_id(tenant_id)
            .await?
            .ok_or(BillingError::TenantNotFound(tenant_id))?;

        Ok((tenant.plan_tier.clone(), tenant.plan_tier.limits()))
    }

    // -----------------------------------------------------------------------
    // Budget guard
    // -----------------------------------------------------------------------

    /// Return `Ok(())` if the tenant has remaining AI token budget for this
    /// calendar month, or `Err(BillingError::BudgetExceeded)` otherwise.
    ///
    /// Scale tenants have no practical limit (`u64::MAX`) and always pass.
    pub async fn check_ai_budget(&self, tenant_id: TenantId) -> Result<(), BillingError> {
        let (_, limits) = self.get_plan(tenant_id).await?;

        // Scale tier: no cap.
        if limits.monthly_ai_tokens == u64::MAX {
            return Ok(());
        }

        let used = self.token_usage.get_monthly_usage(tenant_id).await?;

        if used >= limits.monthly_ai_tokens {
            return Err(BillingError::BudgetExceeded {
                used,
                limit: limits.monthly_ai_tokens,
            });
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Admin override
    // -----------------------------------------------------------------------

    /// Directly set a tenant's plan tier (admin-only endpoint).
    ///
    /// Both `admin_secret` (the value from the HTTP header) and
    /// `configured_secret` (from `AdminConfig`) must be non-empty and equal.
    pub async fn set_plan(
        &self,
        admin_secret: &str,
        configured_secret: &str,
        tenant_id: TenantId,
        new_plan: PlanTier,
    ) -> Result<Tenant, BillingError> {
        // Reject if the admin secret is disabled (empty) or doesn't match.
        if configured_secret.is_empty() || admin_secret != configured_secret {
            return Err(BillingError::Forbidden);
        }

        // Verify the tenant exists before attempting the update.
        self.tenants
            .find_by_id(tenant_id)
            .await?
            .ok_or(BillingError::TenantNotFound(tenant_id))?;

        // Persist the override — don't touch the Stripe customer ID.
        let updated = self
            .tenants
            .update_plan(tenant_id, new_plan.clone(), None)
            .await?;

        tracing::info!(tenant_id = %tenant_id, plan = %new_plan, "Admin plan override");

        Ok(updated)
    }

    // -----------------------------------------------------------------------
    // Admin helpers
    // -----------------------------------------------------------------------

    /// Return this month's AI token usage for a tenant (used by admin endpoints).
    pub async fn current_token_usage(&self, tenant_id: TenantId) -> Result<u64, BillingError> {
        Ok(self.token_usage.get_monthly_usage(tenant_id).await?)
    }

    /// Fetch a tenant by ID (used by admin GET endpoint).
    pub async fn find_tenant(&self, tenant_id: TenantId) -> Result<Tenant, BillingError> {
        self.tenants
            .find_by_id(tenant_id)
            .await?
            .ok_or(BillingError::TenantNotFound(tenant_id))
    }

    // -----------------------------------------------------------------------
    // Background audit
    // -----------------------------------------------------------------------

    /// Scan all tenants and emit warnings for those approaching or over their
    /// monthly AI token budget.
    ///
    /// Called once per day from the server's background task scheduler.
    /// Never returns an error — failures are logged and the audit continues.
    pub async fn run_usage_audit(&self) {
        let tenants = match self.tenants.list_all().await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = ?e, "Billing audit: failed to list tenants");
                return;
            }
        };

        for tenant in tenants {
            let limits = tenant.plan_tier.limits();
            if limits.monthly_ai_tokens == u64::MAX {
                continue; // Scale tier has no limit
            }
            let used = match self.token_usage.get_monthly_usage(tenant.id).await {
                Ok(u) => u,
                Err(e) => {
                    tracing::warn!(
                        tenant_id = %tenant.id,
                        error = ?e,
                        "Billing audit: could not read token usage"
                    );
                    continue;
                }
            };
            #[allow(clippy::cast_possible_truncation)]
            let pct = (used * 100) / limits.monthly_ai_tokens;
            if pct >= 100 {
                tracing::warn!(
                    tenant_id  = %tenant.id,
                    plan       = %tenant.plan_tier,
                    used       = used,
                    limit      = limits.monthly_ai_tokens,
                    "AI token budget EXCEEDED"
                );
            } else if pct >= 80 {
                tracing::warn!(
                    tenant_id  = %tenant.id,
                    plan       = %tenant.plan_tier,
                    used       = used,
                    limit      = limits.monthly_ai_tokens,
                    pct        = pct,
                    "AI token budget at {}% — approaching limit", pct
                );
            }
        }
        tracing::info!("Billing usage audit completed");
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use chrono::Utc;

    use forgebike_config::StripeConfig;
    use forgebike_domain::{
        entities::tenant::{PlanTier, Tenant},
        identifiers::TenantId,
        ports::{
            billing_port::BillingPort,
            tenant_repository::{NewTenant, TenantRepository},
            token_usage_store::TokenUsageStore,
        },
        DomainError,
    };

    use super::{super::error::BillingError, BillingService};

    // -- Mock: TenantRepository ----------------------------------------------

    struct MockTenants(Mutex<Vec<Tenant>>);

    impl MockTenants {
        fn new() -> Arc<Self> {
            Arc::new(Self(Mutex::new(vec![])))
        }

        fn add(&self, tenant: Tenant) {
            self.0.lock().unwrap().push(tenant);
        }
    }

    #[async_trait]
    impl TenantRepository for MockTenants {
        async fn create(&self, new_tenant: NewTenant) -> Result<Tenant, DomainError> {
            let t = Tenant {
                id: TenantId::new(),
                name: new_tenant.name,
                plan_tier: PlanTier::Starter,
                stripe_customer_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.0.lock().unwrap().push(t.clone());
            Ok(t)
        }

        async fn find_by_id(&self, id: TenantId) -> Result<Option<Tenant>, DomainError> {
            Ok(self.0.lock().unwrap().iter().find(|t| t.id == id).cloned())
        }

        async fn update_plan(
            &self,
            id: TenantId,
            plan: PlanTier,
            stripe_customer_id: Option<&str>,
        ) -> Result<Tenant, DomainError> {
            let mut guard = self.0.lock().unwrap();
            let tenant = guard
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or_else(|| DomainError::NotFound(format!("tenant {id}")))?;
            tenant.plan_tier = plan;
            if let Some(cid) = stripe_customer_id {
                tenant.stripe_customer_id = Some(cid.to_string());
            }
            tenant.updated_at = Utc::now();
            Ok(tenant.clone())
        }

        async fn find_by_stripe_customer_id(
            &self,
            stripe_customer_id: &str,
        ) -> Result<Option<Tenant>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.stripe_customer_id.as_deref() == Some(stripe_customer_id))
                .cloned())
        }

        async fn list_all(&self) -> Result<Vec<Tenant>, DomainError> {
            Ok(self.0.lock().unwrap().clone())
        }
    }

    // -- Mock: BillingPort ---------------------------------------------------

    struct MockBilling;

    // No #[async_trait] needed — verify_webhook_signature is synchronous.
    impl BillingPort for MockBilling {
        fn verify_webhook_signature(
            &self,
            _payload: &[u8],
            _stripe_signature: &str,
        ) -> Result<(), DomainError> {
            Ok(())
        }
    }

    // -- Mock: TokenUsageStore -----------------------------------------------

    struct MockTokens(Mutex<u64>);

    impl MockTokens {
        fn new() -> Arc<Self> {
            Arc::new(Self(Mutex::new(0)))
        }

        fn with_usage(initial: u64) -> Arc<Self> {
            Arc::new(Self(Mutex::new(initial)))
        }
    }

    #[async_trait]
    impl TokenUsageStore for MockTokens {
        async fn record_usage(&self, _tid: TenantId, tokens: u64) -> Result<u64, DomainError> {
            let mut guard = self.0.lock().unwrap();
            *guard += tokens;
            Ok(*guard)
        }

        async fn get_monthly_usage(&self, _tid: TenantId) -> Result<u64, DomainError> {
            Ok(*self.0.lock().unwrap())
        }
    }

    // -- Helpers -------------------------------------------------------------

    fn make_stripe_config() -> StripeConfig {
        StripeConfig {
            webhook_secret: "whsec_test".into(),
            price_id_growth: "price_growth".into(),
            price_id_scale: "price_scale".into(),
        }
    }

    fn make_service(
        tenants: Arc<MockTenants>,
        billing: Arc<MockBilling>,
        tokens: Arc<MockTokens>,
    ) -> BillingService {
        BillingService::new(
            tenants as _,
            billing as _,
            tokens as _,
            make_stripe_config(),
        )
    }

    /// Insert a Starter tenant directly into the mock store and return a clone.
    fn add_starter_tenant(store: &MockTenants) -> Tenant {
        let t = Tenant {
            id: TenantId::new(),
            name: "Test Restaurant".into(),
            plan_tier: PlanTier::Starter,
            stripe_customer_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        store.add(t.clone());
        t
    }

    // -- Tests ---------------------------------------------------------------

    /// Usage well under the Starter limit (10 000 tokens) → Ok.
    #[tokio::test]
    async fn check_ai_budget_ok() {
        let tenants = MockTenants::new();
        let tenant = add_starter_tenant(&tenants);
        let svc = make_service(
            tenants,
            Arc::new(MockBilling),
            MockTokens::with_usage(5_000),
        );

        svc.check_ai_budget(tenant.id).await.unwrap();
    }

    /// Usage exactly at the Starter limit → `BudgetExceeded`.
    #[tokio::test]
    async fn check_ai_budget_exceeded() {
        let tenants = MockTenants::new();
        let tenant = add_starter_tenant(&tenants);
        let svc = make_service(
            tenants,
            Arc::new(MockBilling),
            MockTokens::with_usage(10_000),
        );

        let err = svc.check_ai_budget(tenant.id).await.unwrap_err();
        assert!(
            matches!(
                err,
                BillingError::BudgetExceeded {
                    used: 10_000,
                    limit: 10_000
                }
            ),
            "expected BudgetExceeded, got {err}"
        );
    }

    /// Scale tier has `u64::MAX` tokens — always passes regardless of usage.
    #[tokio::test]
    async fn check_ai_budget_scale_no_limit() {
        let tenants = MockTenants::new();
        let scale_tenant = Tenant {
            id: TenantId::new(),
            name: "Scale Co".into(),
            plan_tier: PlanTier::Scale,
            stripe_customer_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        tenants.add(scale_tenant.clone());

        // Even with enormous recorded usage, Scale tenants are never blocked.
        let svc = make_service(
            tenants,
            Arc::new(MockBilling),
            MockTokens::with_usage(999_999_999),
        );

        svc.check_ai_budget(scale_tenant.id).await.unwrap();
    }

    /// Providing the wrong admin secret returns Forbidden.
    #[tokio::test]
    async fn set_plan_wrong_secret_forbidden() {
        let tenants = MockTenants::new();
        let tenant = add_starter_tenant(&tenants);
        let svc = make_service(tenants, Arc::new(MockBilling), MockTokens::new());

        let err = svc
            .set_plan("wrong-key", "correct-key", tenant.id, PlanTier::Growth)
            .await
            .unwrap_err();

        assert!(
            matches!(err, BillingError::Forbidden),
            "expected Forbidden, got {err}"
        );
    }

    /// Correct admin secret updates the tenant's plan and returns the new tier.
    #[tokio::test]
    async fn set_plan_ok() {
        let tenants = MockTenants::new();
        let tenant = add_starter_tenant(&tenants);
        let svc = make_service(tenants, Arc::new(MockBilling), MockTokens::new());

        let updated = svc
            .set_plan("admin-secret", "admin-secret", tenant.id, PlanTier::Growth)
            .await
            .unwrap();

        assert_eq!(updated.plan_tier, PlanTier::Growth);
    }
}
