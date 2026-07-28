//! `sqlx`-backed implementation of [`MenuItemRepository`].

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use forgebike_domain::{
    entities::menu_item::MenuItem,
    identifiers::{MenuItemId, RestaurantId, TenantId},
    pagination::{Cursor, ListParams, Page},
    ports::menu_item_repository::{MenuItemRepository, NewMenuItem},
    DomainError,
};

// ---------------------------------------------------------------------------
// DB row
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct MenuItemRow {
    id: Uuid,
    restaurant_id: Uuid,
    tenant_id: Uuid,
    name: String,
    description: Option<String>,
    price_cents: Option<i64>,
    category: Option<String>,
    is_available: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<MenuItemRow> for MenuItem {
    fn from(r: MenuItemRow) -> Self {
        Self {
            id: MenuItemId::from_uuid(r.id),
            restaurant_id: RestaurantId::from_uuid(r.restaurant_id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            name: r.name,
            description: r.description,
            price_cents: r.price_cents,
            category: r.category,
            is_available: r.is_available,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

pub struct PgMenuItemRepository {
    pool: PgPool,
}

impl PgMenuItemRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MenuItemRepository for PgMenuItemRepository {
    async fn create(&self, new: NewMenuItem) -> Result<MenuItem, DomainError> {
        let row = sqlx::query_as::<_, MenuItemRow>(
            r"
            INSERT INTO menu_items
                (restaurant_id, tenant_id, name, description, price_cents, category, is_available)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING
                id, restaurant_id, tenant_id, name, description,
                price_cents, category, is_available, created_at, updated_at
            ",
        )
        .bind(new.restaurant_id.as_uuid())
        .bind(new.tenant_id.as_uuid())
        .bind(&new.name)
        .bind(&new.description)
        .bind(new.price_cents)
        .bind(&new.category)
        .bind(new.is_available)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(row.into())
    }

    async fn find_by_id(
        &self,
        tenant_id: TenantId,
        id: MenuItemId,
    ) -> Result<Option<MenuItem>, DomainError> {
        let row = sqlx::query_as::<_, MenuItemRow>(
            r"
            SELECT
                id, restaurant_id, tenant_id, name, description,
                price_cents, category, is_available, created_at, updated_at
            FROM menu_items
            WHERE tenant_id = $1 AND id = $2
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(row.map(Into::into))
    }

    async fn list(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        params: ListParams,
    ) -> Result<Page<MenuItem>, DomainError> {
        let cursor = params.cursor.unwrap_or_else(Cursor::start);
        let fetch_limit = params.limit + 1;

        let rows = sqlx::query_as::<_, MenuItemRow>(
            r"
            SELECT
                id, restaurant_id, tenant_id, name, description,
                price_cents, category, is_available, created_at, updated_at
            FROM menu_items
            WHERE tenant_id = $1
              AND restaurant_id = $2
              AND (created_at, id) > ($3, $4)
            ORDER BY created_at ASC, id ASC
            LIMIT $5
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(cursor.created_at)
        .bind(cursor.id)
        .bind(fetch_limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        build_page(rows, params.limit)
    }

    async fn update(&self, item: &MenuItem) -> Result<MenuItem, DomainError> {
        let row = sqlx::query_as::<_, MenuItemRow>(
            r"
            UPDATE menu_items
            SET
                name         = $3,
                description  = $4,
                price_cents  = $5,
                category     = $6,
                is_available = $7
            WHERE id = $1 AND tenant_id = $2
            RETURNING
                id, restaurant_id, tenant_id, name, description,
                price_cents, category, is_available, created_at, updated_at
            ",
        )
        .bind(item.id.as_uuid())
        .bind(item.tenant_id.as_uuid())
        .bind(&item.name)
        .bind(&item.description)
        .bind(item.price_cents)
        .bind(&item.category)
        .bind(item.is_available)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(row.into())
    }

    async fn delete(&self, tenant_id: TenantId, id: MenuItemId) -> Result<bool, DomainError> {
        let result = sqlx::query("DELETE FROM menu_items WHERE id = $1 AND tenant_id = $2")
            .bind(id.as_uuid())
            .bind(tenant_id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }
}

// ---------------------------------------------------------------------------
// Pagination helper
// ---------------------------------------------------------------------------

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::unnecessary_wraps
)]
fn build_page(mut rows: Vec<MenuItemRow>, limit: i64) -> Result<Page<MenuItem>, DomainError> {
    let limit_usize = usize::try_from(limit).unwrap_or(usize::MAX);
    let has_more = rows.len() > limit_usize;
    if has_more {
        rows.truncate(limit_usize);
    }

    let next_cursor = if has_more {
        rows.last().map(|r| Cursor {
            created_at: r.created_at,
            id: r.id,
        })
    } else {
        None
    };

    Ok(Page {
        items: rows.into_iter().map(Into::into).collect(),
        next_cursor,
    })
}
