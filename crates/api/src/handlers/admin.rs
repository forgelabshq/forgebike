//! Admin endpoint handlers — cross-tenant plan management.
//!
//! ## Authentication
//! All admin endpoints require the `X-Admin-Key: <secret>` header.
//! The secret is configured via `APP__ADMIN__SECRET_KEY`.
//! When the configured secret is empty, all admin endpoints return `403`.
//!
//! ## Endpoints
//! | Method | Path | Description |
//! |---|---|---|
//! | `GET`   | `/api/v1/admin/tenants/:id/plan` | Get a tenant's current plan + usage |
//! | `PATCH` | `/api/v1/admin/tenants/:id/plan` | Override a tenant's plan tier |

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgebike_domain::{
    entities::tenant::{PlanLimits, PlanTier, Tenant},
    identifiers::TenantId,
};

use crate::{
    error::{ApiError, ApiResult},
    extractors::ValidatedJson,
    state::AppState,
};

// ---------------------------------------------------------------------------
// Response DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct TenantPlanResponse {
    pub tenant_id: String,
    pub tenant_name: String,
    pub plan_tier: String,
    pub limits: PlanLimitsDto,
    /// Monthly AI tokens used this calendar month.
    pub tokens_used: u64,
}

#[derive(Serialize)]
pub struct PlanLimitsDto {
    pub monthly_ai_tokens: String, // "unlimited" | number
    pub max_restaurants: u32,
    pub max_contacts_per_restaurant: u32,
    pub campaigns_enabled: bool,
}

impl From<PlanLimits> for PlanLimitsDto {
    fn from(l: PlanLimits) -> Self {
        Self {
            monthly_ai_tokens: if l.monthly_ai_tokens == u64::MAX {
                "unlimited".into()
            } else {
                l.monthly_ai_tokens.to_string()
            },
            max_restaurants: l.max_restaurants,
            max_contacts_per_restaurant: l.max_contacts_per_restaurant,
            campaigns_enabled: l.campaigns_enabled,
        }
    }
}

// ---------------------------------------------------------------------------
// Request DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, validator::Validate)]
pub struct SetPlanBody {
    /// Target plan tier: `"starter"` | `"growth"` | `"scale"`
    #[validate(length(min = 1))]
    pub plan: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn extract_admin_key(headers: &HeaderMap) -> &str {
    headers
        .get("x-admin-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

fn parse_plan(s: &str) -> ApiResult<PlanTier> {
    s.parse::<PlanTier>().map_err(|_| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("unknown plan tier: {s:?}. Use: starter, growth, scale"),
        )
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/admin/tenants/:id/plan`
///
/// Returns the current plan tier, its limits, and this month's AI token usage.
/// Requires `X-Admin-Key` header.
#[tracing::instrument(skip(state, headers), name = "handlers::admin::get_plan")]
pub async fn get_tenant_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tenant_id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let admin_key = extract_admin_key(&headers);
    if state.config.admin.secret_key.is_empty() || admin_key != state.config.admin.secret_key {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "Valid X-Admin-Key header required",
        ));
    }

    let tid = TenantId::from_uuid(tenant_id);
    let (plan, limits) = state
        .billing_service
        .get_plan(tid)
        .await
        .map_err(ApiError::from)?;

    let tokens_used = state
        .billing_service
        .current_token_usage(tid)
        .await
        .map_err(ApiError::from)?;

    let tenant = state
        .billing_service
        .find_tenant(tid)
        .await
        .map_err(ApiError::from)?;

    Ok(Json(TenantPlanResponse {
        tenant_id: tid.to_string(),
        tenant_name: tenant.name,
        plan_tier: plan.to_string(),
        limits: PlanLimitsDto::from(limits),
        tokens_used,
    }))
}

/// `PATCH /api/v1/admin/tenants/:id/plan`
///
/// Manually override a tenant's plan tier.  Useful for internal ops and
/// grace-period upgrades.  Requires `X-Admin-Key` header.
#[tracing::instrument(skip(state, headers, body), name = "handlers::admin::set_plan")]
pub async fn set_tenant_plan(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tenant_id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<SetPlanBody>,
) -> ApiResult<impl IntoResponse> {
    let admin_key = extract_admin_key(&headers);
    let new_plan = parse_plan(&body.plan)?;

    let tenant = state
        .billing_service
        .set_plan(
            admin_key,
            &state.config.admin.secret_key,
            TenantId::from_uuid(tenant_id),
            new_plan,
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(TenantPlanSummary::from(tenant)))
}

// ---------------------------------------------------------------------------
// Summary DTO (returned by set_plan)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct TenantPlanSummary {
    pub tenant_id: String,
    pub name: String,
    pub plan_tier: String,
    pub limits: PlanLimitsDto,
}

impl From<Tenant> for TenantPlanSummary {
    fn from(t: Tenant) -> Self {
        let limits = t.plan_tier.limits();
        Self {
            tenant_id: t.id.to_string(),
            name: t.name,
            plan_tier: t.plan_tier.to_string(),
            limits: PlanLimitsDto::from(limits),
        }
    }
}
