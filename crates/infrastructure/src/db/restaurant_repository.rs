//! `sqlx`-backed implementation of [`RestaurantRepository`].
//!
//! ## Cursor pagination
//!
//! All list queries order by `(created_at ASC, id ASC)` and filter with
//! `(created_at, id) > (cursor.created_at, cursor.id)`.  For the first page
//! the cursor is `Cursor::start()` (Unix epoch + nil UUID), which is less
//! than every real row.
//!
//! The `n + 1` trick: we fetch `limit + 1` rows and return only `limit` to
//! the caller.  If we received the extra row it becomes the `next_cursor`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use forgebike_domain::{
    entities::restaurant::Restaurant,
    identifiers::{RestaurantId, TenantId},
    pagination::{Cursor, ListParams, Page},
    ports::restaurant_repository::{NewRestaurant, RestaurantRepository},
    DomainError,
};

// ---------------------------------------------------------------------------
// DB row (private — never crosses the infrastructure boundary)
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct RestaurantRow {
    id: Uuid,
    tenant_id: Uuid,
    name: String,
    description: Option<String>,
    cuisine_type: Option<String>,
    address: Option<String>,
    phone: Option<String>,
    website: Option<String>,
    google_place_id: Option<String>,
    yelp_id: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<RestaurantRow> for Restaurant {
    fn from(r: RestaurantRow) -> Self {
        Self {
            id: RestaurantId::from_uuid(r.id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            name: r.name,
            description: r.description,
            cuisine_type: r.cuisine_type,
            address: r.address,
            phone: r.phone,
            website: r.website,
            google_place_id: r.google_place_id,
            yelp_id: r.yelp_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

pub struct PgRestaurantRepository {
    pool: PgPool,
}

impl PgRestaurantRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RestaurantRepository for PgRestaurantRepository {
    async fn create(&self, new: NewRestaurant) -> Result<Restaurant, DomainError> {
        let row = sqlx::query_as::<_, RestaurantRow>(
            r"
            INSERT INTO restaurants
                (tenant_id, name, description, cuisine_type, address, phone, website)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING
                id, tenant_id, name, description, cuisine_type,
                address, phone, website, google_place_id, yelp_id,
                created_at, updated_at
            ",
        )
        .bind(new.tenant_id.as_uuid())
        .bind(&new.name)
        .bind(&new.description)
        .bind(&new.cuisine_type)
        .bind(&new.address)
        .bind(&new.phone)
        .bind(&new.website)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(row.into())
    }

    async fn find_by_id(
        &self,
        tenant_id: TenantId,
        id: RestaurantId,
    ) -> Result<Option<Restaurant>, DomainError> {
        let row = sqlx::query_as::<_, RestaurantRow>(
            r"
            SELECT
                id, tenant_id, name, description, cuisine_type,
                address, phone, website, google_place_id, yelp_id,
                created_at, updated_at
            FROM restaurants
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
        params: ListParams,
    ) -> Result<Page<Restaurant>, DomainError> {
        let cursor = params.cursor.unwrap_or_else(Cursor::start);
        // Fetch one extra row to detect whether a next page exists.
        let fetch_limit = params.limit + 1;

        let rows = sqlx::query_as::<_, RestaurantRow>(
            r"
            SELECT
                id, tenant_id, name, description, cuisine_type,
                address, phone, website, google_place_id, yelp_id,
                created_at, updated_at
            FROM restaurants
            WHERE tenant_id = $1
              AND (created_at, id) > ($2, $3)
            ORDER BY created_at ASC, id ASC
            LIMIT $4
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(cursor.created_at)
        .bind(cursor.id)
        .bind(fetch_limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        build_page(rows, params.limit)
    }

    async fn update(&self, restaurant: &Restaurant) -> Result<Restaurant, DomainError> {
        let row = sqlx::query_as::<_, RestaurantRow>(
            r"
            UPDATE restaurants
            SET
                name            = $3,
                description     = $4,
                cuisine_type    = $5,
                address         = $6,
                phone           = $7,
                website         = $8,
                google_place_id = $9,
                yelp_id         = $10
            WHERE id = $1 AND tenant_id = $2
            RETURNING
                id, tenant_id, name, description, cuisine_type,
                address, phone, website, google_place_id, yelp_id,
                created_at, updated_at
            ",
        )
        .bind(restaurant.id.as_uuid())
        .bind(restaurant.tenant_id.as_uuid())
        .bind(&restaurant.name)
        .bind(&restaurant.description)
        .bind(&restaurant.cuisine_type)
        .bind(&restaurant.address)
        .bind(&restaurant.phone)
        .bind(&restaurant.website)
        .bind(&restaurant.google_place_id)
        .bind(&restaurant.yelp_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(row.into())
    }

    async fn delete(&self, tenant_id: TenantId, id: RestaurantId) -> Result<bool, DomainError> {
        let result = sqlx::query("DELETE FROM restaurants WHERE id = $1 AND tenant_id = $2")
            .bind(id.as_uuid())
            .bind(tenant_id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }
}

// ---------------------------------------------------------------------------
// Shared pagination helper
// ---------------------------------------------------------------------------

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::unnecessary_wraps
)]
fn build_page(mut rows: Vec<RestaurantRow>, limit: i64) -> Result<Page<Restaurant>, DomainError> {
    // Compare lengths without casting: fetch_limit = limit + 1, so if we got
    // more than `limit` items, there are more pages.
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
