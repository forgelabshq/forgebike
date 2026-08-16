# Phase 8 — Billing & Subscription

> **Status**: Complete  
> **Timeframe**: Week 13  
> **Exit criterion**: A tenant can be moved between plan tiers and feature access is correctly gated; AI token budget enforced with 402 responses; daily audit background task warns at 80% and 100% usage; 5 unit tests pass.

---

## Overview

Phase 8 implements the full subscription lifecycle for the Forgebike platform: Stripe webhook processing to update plan tiers automatically, plan-based feature gating, AI token budget enforcement, admin overrides for manual plan management, and a daily background audit job that warns when tenants approach or exceed their AI token limits.

---

## Architecture

```
Stripe → POST /api/v1/billing/webhook
              │
              ▼
         BillingService
           • verify HMAC-SHA256 signature (StripeClient)
           • parse event JSON
           • map price_id → PlanTier
           • update tenant via TenantRepository
              │
              ▼
         PgTenantRepository
           • UPDATE tenants SET plan_tier = $2::plan_tier
```

### Design decisions

| Decision | Rationale |
|---|---|
| No Stripe SDK | Avoids a heavy dependency; HMAC-SHA256 verification is ~40 lines with `hmac` + `sha2` |
| Admin key instead of admin role | Cross-tenant ops need a super-admin credential; adding a new DB role was out of scope |
| Budget check at handler level | Keeps services decoupled; one explicit call per AI handler |
| `u64::MAX` for Scale "unlimited" | Avoids a separate branch — `u64::MAX` tokens is effectively unlimited in practice |
| Daily audit as `tokio::spawn` loop | Achieves the exit criterion without introducing apalis; can be migrated to a proper job queue later |
| Dev bypass when `webhook_secret` is empty | CI has no Stripe keys; empty secret = accept all events |

---

## Plan Tiers

| Tier | Monthly AI Tokens | Max Restaurants | Max Contacts/Restaurant | Campaigns |
|---|---|---|---|---|
| Starter (default) | 10,000 | 1 | 500 | ✗ |
| Growth | 100,000 | 5 | 5,000 | ✓ |
| Scale | Unlimited | 20 | 50,000 | ✓ |

Plan tier is stored as a `plan_tier` PostgreSQL enum on the `tenants` table (introduced in `20250101000001_create_tenants.sql`). Limits are computed at runtime from a `PlanLimits` struct; no limits table is needed.

---

## Endpoints

| Method | Path | Auth | Description |
|---|---|---|---|
| `POST` | `/api/v1/billing/webhook` | Stripe-Signature header | Process Stripe subscription events |
| `GET` | `/api/v1/admin/tenants/:id/plan` | X-Admin-Key header | Get plan tier + usage |
| `PATCH` | `/api/v1/admin/tenants/:id/plan` | X-Admin-Key header | Override plan tier |

---

## Stripe Webhook Event Handling

The webhook handler accepts `POST /api/v1/billing/webhook` with the raw request body and a `Stripe-Signature` header. After signature verification the event type is matched:

| Stripe event | Action |
|---|---|
| `customer.subscription.created` | Map `price.id` to `PlanTier`, call `update_plan` |
| `customer.subscription.updated` | Map `price.id` to `PlanTier`, call `update_plan` |
| `customer.subscription.deleted` | Downgrade tenant to `Starter` |
| All other events | Return `200 OK`, no action (idempotent) |

The `price_id` → `PlanTier` mapping is driven by configuration:

```toml
[stripe]
price_id_growth = "price_xxx"   # Growth tier price ID from Stripe dashboard
price_id_scale  = "price_xxx"   # Scale tier price ID from Stripe dashboard
```

An unrecognised `price_id` logs a warning and returns `200 OK` so Stripe does not retry.

---

## Webhook Verification Algorithm

Stripe signs every webhook delivery with an HMAC-SHA256 digest over the raw body. The verification steps are:

1. Parse `t=<timestamp>,v1=<hmac>` from the `Stripe-Signature` header.
2. Reject if `|now − t| > 300 seconds` — replay protection window.
3. Compute `HMAC-SHA256(webhook_secret, "<t>.<raw_body>")`.
4. Timing-safe compare the computed digest with the `v1` value.

When `webhook_secret` is empty (dev / CI environment) the signature check is bypassed and all events are accepted. This is logged at `WARN` level on startup.

---

## Admin Endpoints

Admin endpoints are protected by a static shared secret passed in the `X-Admin-Key` request header. When `admin.secret_key` is empty in configuration the endpoints return `503 Service Unavailable` immediately.

### `GET /api/v1/admin/tenants/{uuid}/plan`

Returns the current plan tier and monthly AI token usage for any tenant.

**Request:**
```
GET /api/v1/admin/tenants/{uuid}/plan
X-Admin-Key: <secret>
```

**Response `200 OK`:**
```json
{
  "tenant_id": "550e8400-e29b-41d4-a716-446655440000",
  "tenant_name": "Acme Restaurant",
  "plan_tier": "starter",
  "limits": {
    "monthly_ai_tokens": "10000",
    "max_restaurants": 1,
    "max_contacts_per_restaurant": 500,
    "campaigns_enabled": false
  },
  "tokens_used": 1234
}
```

`monthly_ai_tokens` is serialised as a string because the Scale tier uses `u64::MAX`; JSON numbers cannot safely represent that value in all clients.

**Error responses:**

| Status | Trigger |
|---|---|
| `401 Unauthorized` | Missing or incorrect `X-Admin-Key` |
| `404 Not Found` | Tenant UUID does not exist |
| `503 Service Unavailable` | `admin.secret_key` is not configured |

---

### `PATCH /api/v1/admin/tenants/{uuid}/plan`

Overrides the plan tier for any tenant, bypassing Stripe.

**Request:**
```
PATCH /api/v1/admin/tenants/{uuid}/plan
X-Admin-Key: <secret>
Content-Type: application/json

{"plan": "growth"}
```

Accepted values for `plan`: `"starter"`, `"growth"`, `"scale"`.

**Response `200 OK`:** same shape as the GET response, reflecting the updated tier.

---

## AI Budget Enforcement

`billing_service.check_ai_budget(tenant_id)` is called at the top of every handler that consumes OpenAI tokens:

- `POST /api/v1/restaurants/:id/reviews/analyse`
- `POST /api/v1/restaurants/:id/reviews/:rid/reply-draft`
- `POST /api/v1/restaurants/:id/content/generate`
- `GET  /api/v1/restaurants/:id/content/stream`

The check reads the current month's token counter from Redis (`ai:tokens:{tenant_id}:{YYYYMM}`) and compares it against the plan's `monthly_ai_tokens` limit. When the budget is exceeded the handler returns immediately:

```
HTTP 402 Payment Required
{
  "error": "AI token budget exceeded for your current plan. Upgrade to continue."
}
```

The `Scale` tier sets `monthly_ai_tokens = u64::MAX`, so the comparison always passes and Scale tenants are never blocked.

---

## Daily Audit Task

A background loop is spawned at server startup via `tokio::spawn`. It performs a one-minute startup delay (to allow the server to fully initialise), then runs every 24 hours.

On each iteration the task:

1. Lists all active tenants from the database.
2. Skips `Scale` tenants (no cap).
3. For each remaining tenant, reads the current month's token count from Redis.
4. Logs `WARN` if usage is ≥ 80% of the plan limit: `"Tenant {id} at {pct}% of AI token budget ({used}/{limit})"`.
5. Logs `WARN` if usage is ≥ 100%: `"Tenant {id} has exceeded AI token budget ({used}/{limit})"`.

This implements `AuditPlanUsageJob` without introducing apalis. The loop can be replaced by a proper job queue in Phase 9.

---

## Configuration

Add the following sections to `config/default.toml`:

```toml
[stripe]
webhook_secret  = ""   # whsec_xxx — empty = dev bypass
price_id_growth = ""   # price_xxx from Stripe dashboard
price_id_scale  = ""   # price_xxx from Stripe dashboard

[admin]
secret_key = ""   # X-Admin-Key value — empty = admin endpoints disabled
```

Environment variable equivalents (for production / Docker):

```
APP__STRIPE__WEBHOOK_SECRET=whsec_xxxxx
APP__STRIPE__PRICE_ID_GROWTH=price_xxxxx
APP__STRIPE__PRICE_ID_SCALE=price_xxxxx
APP__ADMIN__SECRET_KEY=your-strong-secret-here
```

---

## Unit Tests

All tests live in `crates/application/src/billing/service.rs` and use an in-memory mock `TenantRepository` — no database or network required.

| Test | Covers |
|---|---|
| `check_ai_budget_ok` | Usage under the plan limit passes without error |
| `check_ai_budget_exceeded` | Usage at or above the limit returns `BudgetExceeded` |
| `check_ai_budget_scale_no_limit` | Scale tier always passes regardless of token count |
| `set_plan_wrong_secret_forbidden` | Wrong `X-Admin-Key` value returns `Forbidden` |
| `set_plan_ok` | Correct admin key updates the tenant's plan tier |

Run with:

```bash
cargo test -p forgebike-application billing
```

---

## Files Added / Modified

| File | Change |
|---|---|
| `migrations/20250101000009_add_plan_tier_to_tenants.sql` | **New** — `ALTER TABLE tenants ADD COLUMN plan_tier plan_tier NOT NULL DEFAULT 'starter'` (if not present from Phase 0) |
| `crates/domain/src/entities/plan.rs` | **New** — `PlanTier` enum, `PlanLimits` struct |
| `crates/domain/src/entities/mod.rs` | Added `pub mod plan` |
| `crates/domain/src/ports/tenant_repository.rs` | **New** — `TenantRepository` port trait (`find_by_id`, `update_plan`, `list_all`) |
| `crates/domain/src/ports/mod.rs` | Added `pub mod tenant_repository` |
| `crates/infrastructure/src/db/tenant_repository.rs` | **New** — `PgTenantRepository` |
| `crates/infrastructure/src/db/mod.rs` | Re-exported `PgTenantRepository` |
| `crates/application/src/billing/service.rs` | **New** — `BillingService` + unit tests |
| `crates/application/src/billing/error.rs` | **New** — `BillingError` (`BudgetExceeded`, `Forbidden`, `TenantNotFound`, etc.) |
| `crates/application/src/billing/mod.rs` | **New** — module declaration |
| `crates/application/src/lib.rs` | Added `pub mod billing` |
| `crates/api/src/handlers/billing.rs` | **New** — `webhook_handler`, `get_plan_handler`, `set_plan_handler` |
| `crates/api/src/handlers/mod.rs` | Added `pub mod billing` |
| `crates/api/src/error.rs` | Added `From<BillingError> for ApiError` (402 for `BudgetExceeded`, 401 for `Forbidden`) |
| `crates/api/src/state.rs` | Added `billing_service: Arc<BillingService>` |
| `crates/api/src/router.rs` | Wired `/billing/webhook` and `/admin/tenants/:id/plan` routes |
| `crates/config/src/lib.rs` | Added `StripeConfig` and `AdminConfig` structs |
| `crates/server/src/main.rs` | Instantiated `PgTenantRepository` + `BillingService`; spawned daily audit task |
| `scripts/test.sh` | Added `test_phase_8` function |
| `documentation/phase-8-billing.md` | **New** — this document |
