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
}
