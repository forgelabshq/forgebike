//! `sqlx`-backed implementation of [`TenantRepository`].

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use forgebike_domain::{
    entities::tenant::{PlanTier, Tenant},
    identifiers::TenantId,
    ports::tenant_repository::{NewTenant, TenantRepository},
    DomainError,
};

// ---------------------------------------------------------------------------
// DB row
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct TenantRow {
    id: Uuid,
    name: String,
    plan_tier: String,
    stripe_customer_id: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<TenantRow> for Tenant {
    type Error = DomainError;

    fn try_from(row: TenantRow) -> Result<Self, Self::Error> {
        let plan_tier = row
            .plan_tier
            .parse::<PlanTier>()
            .map_err(DomainError::Internal)?;

        Ok(Tenant {
            id: TenantId::from_uuid(row.id),
            name: row.name,
            plan_tier,
            stripe_customer_id: row.stripe_customer_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

pub struct PgTenantRepository {
    pool: PgPool,
}

impl PgTenantRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TenantRepository for PgTenantRepository {
    async fn create(&self, new_tenant: NewTenant) -> Result<Tenant, DomainError> {
        let row = sqlx::query_as::<_, TenantRow>(
            r"
            INSERT INTO tenants (name)
            VALUES ($1)
            RETURNING
                id,
                name,
                plan_tier::TEXT AS plan_tier,
                stripe_customer_id,
                created_at,
                updated_at
            ",
        )
        .bind(&new_tenant.name)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        Tenant::try_from(row)
    }

    async fn find_by_id(&self, id: TenantId) -> Result<Option<Tenant>, DomainError> {
        let row = sqlx::query_as::<_, TenantRow>(
            r"
            SELECT
                id,
                name,
                plan_tier::TEXT AS plan_tier,
                stripe_customer_id,
                created_at,
                updated_at
            FROM tenants
            WHERE id = $1
            ",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        row.map(Tenant::try_from).transpose()
    }

    async fn update_plan(
        &self,
        id: TenantId,
        plan: PlanTier,
        stripe_customer_id: Option<&str>,
    ) -> Result<Tenant, DomainError> {
        let row = sqlx::query_as::<_, TenantRow>(
            r"
            UPDATE tenants
            SET plan_tier          = $2::plan_tier,
                stripe_customer_id = COALESCE($3::TEXT, stripe_customer_id),
                updated_at         = NOW()
            WHERE id = $1
            RETURNING id, name, plan_tier::TEXT AS plan_tier, stripe_customer_id, created_at, updated_at
            ",
        )
        .bind(id.as_uuid())
        .bind(plan.to_string())
        .bind(stripe_customer_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        let row = row.ok_or_else(|| DomainError::NotFound(format!("Tenant {id} not found")))?;
        Tenant::try_from(row)
    }

    async fn find_by_stripe_customer_id(
        &self,
        stripe_customer_id: &str,
    ) -> Result<Option<Tenant>, DomainError> {
        let row = sqlx::query_as::<_, TenantRow>(
            r"
            SELECT id, name, plan_tier::TEXT AS plan_tier, stripe_customer_id, created_at, updated_at
            FROM tenants
            WHERE stripe_customer_id = $1
            ",
        )
        .bind(stripe_customer_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        row.map(Tenant::try_from).transpose()
    }

    async fn list_all(&self) -> Result<Vec<Tenant>, DomainError> {
        let rows = sqlx::query_as::<_, TenantRow>(
            r"
            SELECT id, name, plan_tier::TEXT AS plan_tier, stripe_customer_id, created_at, updated_at
            FROM tenants
            ORDER BY created_at ASC
            ",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        rows.into_iter().map(Tenant::try_from).collect()
    }
}
