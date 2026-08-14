//! `sqlx`-backed implementation of [`CustomerContactRepository`].
//!
//! Contacts are listed oldest-first using ascending cursor pagination
//! (`ORDER BY created_at ASC, id ASC`) so new imports are always appended
//! at the end of the list.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use forgebike_domain::{
    entities::customer_contact::CustomerContact,
    identifiers::{CustomerContactId, RestaurantId, TenantId},
    pagination::{Cursor, Page},
    ports::customer_contact_repository::{
        ContactListParams, CustomerContactRepository, NewContact, UpdateContact,
    },
    DomainError,
};

// ---------------------------------------------------------------------------
// DB row
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct ContactRow {
    id: Uuid,
    tenant_id: Uuid,
    restaurant_id: Uuid,
    name: String,
    email: Option<String>,
    phone: Option<String>,
    tags: Vec<String>,
    notes: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<ContactRow> for CustomerContact {
    type Error = DomainError;

    fn try_from(r: ContactRow) -> Result<Self, Self::Error> {
        Ok(CustomerContact {
            id: CustomerContactId::from_uuid(r.id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            restaurant_id: RestaurantId::from_uuid(r.restaurant_id),
            name: r.name,
            email: r.email,
            phone: r.phone,
            tags: r.tags,
            notes: r.notes,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

pub struct PgCustomerContactRepository {
    pool: PgPool,
}

impl PgCustomerContactRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CustomerContactRepository for PgCustomerContactRepository {
    async fn create(&self, contact: NewContact) -> Result<CustomerContact, DomainError> {
        let row = sqlx::query_as::<_, ContactRow>(
            r"
            INSERT INTO customer_contacts
                (tenant_id, restaurant_id, name, email, phone, tags, notes)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING
                id, tenant_id, restaurant_id, name, email, phone, tags, notes,
                created_at, updated_at
            ",
        )
        .bind(contact.tenant_id.as_uuid())
        .bind(contact.restaurant_id.as_uuid())
        .bind(&contact.name)
        .bind(&contact.email)
        .bind(&contact.phone)
        .bind(&contact.tags)
        .bind(&contact.notes)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        CustomerContact::try_from(row)
    }

    async fn find_by_id(
        &self,
        tenant_id: TenantId,
        id: CustomerContactId,
    ) -> Result<Option<CustomerContact>, DomainError> {
        let row = sqlx::query_as::<_, ContactRow>(
            r"
            SELECT id, tenant_id, restaurant_id, name, email, phone, tags, notes,
                   created_at, updated_at
            FROM customer_contacts
            WHERE tenant_id = $1 AND id = $2
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        row.map(CustomerContact::try_from).transpose()
    }

    async fn list(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        params: ContactListParams,
    ) -> Result<Page<CustomerContact>, DomainError> {
        let cursor = params.cursor.unwrap_or_else(Cursor::start);
        let fetch_limit = params.limit + 1;

        let rows = sqlx::query_as::<_, ContactRow>(
            r"
            SELECT id, tenant_id, restaurant_id, name, email, phone, tags, notes,
                   created_at, updated_at
            FROM customer_contacts
            WHERE tenant_id = $1
              AND restaurant_id = $2
              AND ($3::TEXT IS NULL OR $3 = ANY(tags))
              AND (created_at > $4 OR (created_at = $4 AND id > $5))
            ORDER BY created_at ASC, id ASC
            LIMIT $6
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(params.tag.as_deref())
        .bind(cursor.created_at)
        .bind(cursor.id)
        .bind(fetch_limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        build_page(rows, params.limit)
    }

    /// Partial update using COALESCE.
    ///
    /// **Limitation (Phase 7):** explicitly clearing a nullable field to `NULL`
    /// (e.g. `UpdateContact { email: Some(None) }`) is treated the same as
    /// leaving it unchanged.  Full nullable-clear support can be added later
    /// with a dynamically-built SET clause.
    async fn update(
        &self,
        tenant_id: TenantId,
        id: CustomerContactId,
        update: UpdateContact,
    ) -> Result<Option<CustomerContact>, DomainError> {
        let row = sqlx::query_as::<_, ContactRow>(
            r"
            UPDATE customer_contacts SET
                name       = COALESCE($3::TEXT,    name),
                email      = COALESCE($4::TEXT,    email),
                phone      = COALESCE($5::TEXT,    phone),
                tags       = COALESCE($6::TEXT[],  tags),
                notes      = COALESCE($7::TEXT,    notes),
                updated_at = NOW()
            WHERE tenant_id = $1 AND id = $2
            RETURNING
                id, tenant_id, restaurant_id, name, email, phone, tags, notes,
                created_at, updated_at
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(id.as_uuid())
        .bind(update.name.as_deref())
        .bind(update.email.as_ref().and_then(|o| o.as_deref()))
        .bind(update.phone.as_ref().and_then(|o| o.as_deref()))
        .bind(update.tags.clone())
        .bind(update.notes.as_ref().and_then(|o| o.as_deref()))
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        row.map(CustomerContact::try_from).transpose()
    }

    async fn delete(
        &self,
        tenant_id: TenantId,
        id: CustomerContactId,
    ) -> Result<bool, DomainError> {
        let result = sqlx::query("DELETE FROM customer_contacts WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.as_uuid())
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    async fn bulk_create(&self, contacts: Vec<NewContact>) -> Result<usize, DomainError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        let mut inserted = 0usize;

        for contact in contacts {
            let result = sqlx::query(
                r"
                INSERT INTO customer_contacts
                    (tenant_id, restaurant_id, name, email, phone, tags, notes)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (tenant_id, restaurant_id, email)
                WHERE email IS NOT NULL DO NOTHING
                ",
            )
            .bind(contact.tenant_id.as_uuid())
            .bind(contact.restaurant_id.as_uuid())
            .bind(&contact.name)
            .bind(&contact.email)
            .bind(&contact.phone)
            .bind(&contact.tags)
            .bind(&contact.notes)
            .execute(&mut *tx)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

            inserted += usize::try_from(result.rows_affected()).unwrap_or(0);
        }

        tx.commit()
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(inserted)
    }

    async fn list_for_campaign(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        tag_filter: Option<&str>,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<CustomerContact>, DomainError> {
        let rows = sqlx::query_as::<_, ContactRow>(
            r"
            SELECT id, tenant_id, restaurant_id, name, email, phone, tags, notes,
                   created_at, updated_at
            FROM customer_contacts
            WHERE tenant_id = $1
              AND restaurant_id = $2
              AND ($3::TEXT IS NULL OR $3 = ANY(tags))
              AND ($4::TIMESTAMPTZ IS NULL OR created_at >= $4)
            ORDER BY created_at ASC
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(tag_filter)
        .bind(since)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        rows.into_iter()
            .map(CustomerContact::try_from)
            .collect::<Result<Vec<_>, _>>()
    }
}

// ---------------------------------------------------------------------------
// Pagination helper (ascending)
// ---------------------------------------------------------------------------

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::unnecessary_wraps
)]
fn build_page(mut rows: Vec<ContactRow>, limit: i64) -> Result<Page<CustomerContact>, DomainError> {
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
        items: rows
            .into_iter()
            .map(CustomerContact::try_from)
            .collect::<Result<_, _>>()?,
        next_cursor,
    })
}
