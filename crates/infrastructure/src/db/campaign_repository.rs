//! `sqlx`-backed implementation of [`CampaignRepository`].
//!
//! Campaigns are listed oldest-first using ascending cursor pagination.
//! `PostgreSQL` enum columns (`channel`, `status`) are cast to `TEXT` in every
//! SELECT so that the `CampaignRow` struct can derive `sqlx::FromRow` with
//! plain `String` fields — avoiding the need for custom `sqlx::Type` impls.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use forgebike_domain::{
    entities::campaign::{Campaign, CampaignChannel, CampaignStatus},
    identifiers::{CampaignId, RestaurantId, TenantId},
    pagination::{Cursor, Page},
    ports::campaign_repository::{
        CampaignListParams, CampaignRepository, NewCampaign, UpdateCampaign,
    },
    DomainError,
};

// ---------------------------------------------------------------------------
// DB row
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct CampaignRow {
    id: Uuid,
    tenant_id: Uuid,
    restaurant_id: Uuid,
    name: String,
    /// Stored as TEXT via `channel::TEXT AS channel` in every SELECT.
    channel: String,
    /// Stored as TEXT via `status::TEXT AS status` in every SELECT.
    status: String,
    subject: Option<String>,
    body: String,
    tag_filter: Option<String>,
    scheduled_at: Option<DateTime<Utc>>,
    sent_at: Option<DateTime<Utc>>,
    recipients_count: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<CampaignRow> for Campaign {
    type Error = DomainError;

    fn try_from(r: CampaignRow) -> Result<Self, Self::Error> {
        let channel = r
            .channel
            .parse::<CampaignChannel>()
            .map_err(DomainError::Internal)?;
        let status = r
            .status
            .parse::<CampaignStatus>()
            .map_err(DomainError::Internal)?;

        Ok(Campaign {
            id: CampaignId::from_uuid(r.id),
            tenant_id: TenantId::from_uuid(r.tenant_id),
            restaurant_id: RestaurantId::from_uuid(r.restaurant_id),
            name: r.name,
            channel,
            status,
            subject: r.subject,
            body: r.body,
            tag_filter: r.tag_filter,
            scheduled_at: r.scheduled_at,
            sent_at: r.sent_at,
            recipients_count: r.recipients_count,
            created_at: r.created_at,
            updated_at: r.updated_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

pub struct PgCampaignRepository {
    pool: PgPool,
}

impl PgCampaignRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CampaignRepository for PgCampaignRepository {
    async fn create(&self, campaign: NewCampaign) -> Result<Campaign, DomainError> {
        let row = sqlx::query_as::<_, CampaignRow>(
            r"
            INSERT INTO campaigns
                (tenant_id, restaurant_id, name, channel, subject, body, tag_filter, scheduled_at)
            VALUES ($1, $2, $3, $4::campaign_channel, $5, $6, $7, $8)
            RETURNING
                id, tenant_id, restaurant_id, name,
                channel::TEXT AS channel, status::TEXT AS status,
                subject, body, tag_filter, scheduled_at, sent_at,
                recipients_count, created_at, updated_at
            ",
        )
        .bind(campaign.tenant_id.as_uuid())
        .bind(campaign.restaurant_id.as_uuid())
        .bind(&campaign.name)
        .bind(campaign.channel.to_string())
        .bind(&campaign.subject)
        .bind(&campaign.body)
        .bind(&campaign.tag_filter)
        .bind(campaign.scheduled_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        Campaign::try_from(row)
    }

    async fn find_by_id(
        &self,
        tenant_id: TenantId,
        id: CampaignId,
    ) -> Result<Option<Campaign>, DomainError> {
        let row = sqlx::query_as::<_, CampaignRow>(
            r"
            SELECT
                id, tenant_id, restaurant_id, name,
                channel::TEXT AS channel, status::TEXT AS status,
                subject, body, tag_filter, scheduled_at, sent_at,
                recipients_count, created_at, updated_at
            FROM campaigns
            WHERE tenant_id = $1 AND id = $2
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        row.map(Campaign::try_from).transpose()
    }

    async fn list(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
        params: CampaignListParams,
    ) -> Result<Page<Campaign>, DomainError> {
        let cursor = params.cursor.unwrap_or_else(Cursor::start);
        let fetch_limit = params.limit + 1;

        let rows = sqlx::query_as::<_, CampaignRow>(
            r"
            SELECT
                id, tenant_id, restaurant_id, name,
                channel::TEXT AS channel, status::TEXT AS status,
                subject, body, tag_filter, scheduled_at, sent_at,
                recipients_count, created_at, updated_at
            FROM campaigns
            WHERE tenant_id = $1
              AND restaurant_id = $2
              AND ($3::TEXT IS NULL OR status::TEXT = $3)
              AND (created_at > $4 OR (created_at = $4 AND id > $5))
            ORDER BY created_at ASC, id ASC
            LIMIT $6
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(restaurant_id.as_uuid())
        .bind(params.status.as_ref().map(std::string::ToString::to_string))
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
    /// **Limitation (Phase 7):** explicitly clearing `subject`, `tag_filter`,
    /// or `scheduled_at` to `NULL` is treated the same as leaving the field
    /// unchanged.  Full nullable-clear support can be added later with a
    /// dynamically-built SET clause.
    async fn update(
        &self,
        tenant_id: TenantId,
        id: CampaignId,
        update: UpdateCampaign,
    ) -> Result<Option<Campaign>, DomainError> {
        let row = sqlx::query_as::<_, CampaignRow>(
            r"
            UPDATE campaigns SET
                name         = COALESCE($3::TEXT,        name),
                subject      = COALESCE($4::TEXT,        subject),
                body         = COALESCE($5::TEXT,        body),
                tag_filter   = COALESCE($6::TEXT,        tag_filter),
                scheduled_at = COALESCE($7::TIMESTAMPTZ, scheduled_at),
                updated_at   = NOW()
            WHERE tenant_id = $1 AND id = $2
            RETURNING
                id, tenant_id, restaurant_id, name,
                channel::TEXT AS channel, status::TEXT AS status,
                subject, body, tag_filter, scheduled_at, sent_at,
                recipients_count, created_at, updated_at
            ",
        )
        .bind(tenant_id.as_uuid())
        .bind(id.as_uuid())
        .bind(update.name.as_deref())
        .bind(update.subject.as_ref().and_then(|o| o.as_deref()))
        .bind(update.body.as_deref())
        .bind(update.tag_filter.as_ref().and_then(|o| o.as_deref()))
        .bind(update.scheduled_at.flatten())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        row.map(Campaign::try_from).transpose()
    }

    async fn delete(&self, tenant_id: TenantId, id: CampaignId) -> Result<bool, DomainError> {
        let result = sqlx::query("DELETE FROM campaigns WHERE tenant_id = $1 AND id = $2")
            .bind(tenant_id.as_uuid())
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }

    async fn set_status(
        &self,
        id: CampaignId,
        status: CampaignStatus,
        recipients_count: Option<i32>,
        sent_at: Option<DateTime<Utc>>,
    ) -> Result<(), DomainError> {
        sqlx::query(
            r"
            UPDATE campaigns SET
                status           = $2::campaign_status,
                recipients_count = COALESCE($3, recipients_count),
                sent_at          = COALESCE($4, sent_at),
                updated_at       = NOW()
            WHERE id = $1
            ",
        )
        .bind(id.as_uuid())
        .bind(status.to_string())
        .bind(recipients_count)
        .bind(sent_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        Ok(())
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
fn build_page(mut rows: Vec<CampaignRow>, limit: i64) -> Result<Page<Campaign>, DomainError> {
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
            .map(Campaign::try_from)
            .collect::<Result<_, _>>()?,
        next_cursor,
    })
}
