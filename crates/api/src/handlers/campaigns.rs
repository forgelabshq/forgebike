//! Campaign management handlers.
//!
//! ## Endpoints
//! | Method | Path | Description |
//! |---|---|---|
//! | `POST`   | `/api/v1/restaurants/:id/campaigns`         | Create a campaign |
//! | `GET`    | `/api/v1/restaurants/:id/campaigns`         | List campaigns (paginated) |
//! | `GET`    | `/api/v1/restaurants/:id/campaigns/:cid`    | Get a campaign |
//! | `PATCH`  | `/api/v1/restaurants/:id/campaigns/:cid`    | Update a draft campaign |
//! | `DELETE` | `/api/v1/restaurants/:id/campaigns/:cid`    | Delete a draft campaign |
//! | `POST`   | `/api/v1/restaurants/:id/campaigns/:cid/send` | Dispatch a campaign |

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgebike_application::campaign::SendResult;
use forgebike_domain::{
    entities::{
        auth_identity::AuthIdentity,
        campaign::{Campaign, CampaignChannel, CampaignStatus},
    },
    identifiers::{CampaignId, RestaurantId},
    ports::campaign_repository::{CampaignListParams, NewCampaign, UpdateCampaign},
};

use crate::{
    error::{ApiError, ApiResult},
    extractors::ValidatedJson,
    pagination::{decode_cursor, encode_cursor, PageResponse},
    state::AppState,
};

// ---------------------------------------------------------------------------
// Response DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct CampaignResponse {
    pub id: String,
    pub restaurant_id: String,
    pub name: String,
    pub channel: String,
    pub status: String,
    pub subject: Option<String>,
    pub body: String,
    pub tag_filter: Option<String>,
    pub scheduled_at: Option<String>,
    pub sent_at: Option<String>,
    pub recipients_count: i32,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Campaign> for CampaignResponse {
    fn from(c: Campaign) -> Self {
        Self {
            id: c.id.to_string(),
            restaurant_id: c.restaurant_id.to_string(),
            name: c.name,
            channel: c.channel.to_string(),
            status: c.status.to_string(),
            subject: c.subject,
            body: c.body,
            tag_filter: c.tag_filter,
            scheduled_at: c.scheduled_at.map(|t| t.to_rfc3339()),
            sent_at: c.sent_at.map(|t| t.to_rfc3339()),
            recipients_count: c.recipients_count,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Serialize)]
pub struct SendResponse {
    pub campaign_id: String,
    pub status: String,
    pub recipients_count: i32,
    pub channel: String,
}

impl From<SendResult> for SendResponse {
    fn from(r: SendResult) -> Self {
        Self {
            campaign_id: r.campaign_id.to_string(),
            status: "sending".into(),
            recipients_count: r.recipients_count,
            channel: r.channel.to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// Request DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, validator::Validate)]
pub struct CreateCampaignBody {
    #[validate(length(min = 1, max = 200))]
    pub name: String,

    /// `"email"` | `"sms"`
    pub channel: Option<String>,

    #[validate(length(max = 300))]
    pub subject: Option<String>,

    #[validate(length(min = 1))]
    pub body: String,

    /// When set, only contacts with this tag receive the campaign.
    pub tag_filter: Option<String>,

    pub scheduled_at: Option<String>,
}

#[derive(Debug, Deserialize, validator::Validate)]
pub struct UpdateCampaignBody {
    #[validate(length(min = 1, max = 200))]
    pub name: Option<String>,

    #[validate(length(max = 300))]
    pub subject: Option<String>,

    #[validate(length(min = 1))]
    pub body: Option<String>,

    pub tag_filter: Option<String>,
    pub scheduled_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CampaignListQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub cursor: Option<String>,
    /// Filter by status: `draft` | `sending` | `sent` | `failed`
    pub status: Option<String>,
}

fn default_limit() -> i64 {
    20
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_channel(s: &str) -> ApiResult<CampaignChannel> {
    s.parse::<CampaignChannel>().map_err(|_| {
        ApiError::new(
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            format!("unknown channel: {s:?}. Use: email, sms"),
        )
    })
}

fn parse_status(s: &str) -> ApiResult<CampaignStatus> {
    s.parse::<CampaignStatus>().map_err(|_| {
        ApiError::new(
            axum::http::StatusCode::UNPROCESSABLE_ENTITY,
            format!("unknown status: {s:?}. Use: draft, scheduled, sending, sent, failed"),
        )
    })
}

fn parse_rfc3339_opt(s: Option<&str>) -> ApiResult<Option<chrono::DateTime<chrono::Utc>>> {
    s.map(|raw| {
        chrono::DateTime::parse_from_rfc3339(raw)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|_| ApiError::unprocessable(format!("invalid RFC 3339 date-time: {raw:?}")))
    })
    .transpose()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/restaurants/:id/campaigns`
#[tracing::instrument(skip(state, body), name = "handlers::campaigns::create")]
pub async fn create_campaign(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<CreateCampaignBody>,
) -> ApiResult<impl IntoResponse> {
    let channel = parse_channel(body.channel.as_deref().unwrap_or("email"))?;
    let scheduled_at = parse_rfc3339_opt(body.scheduled_at.as_deref())?;
    let rid = RestaurantId::from_uuid(restaurant_id);

    let campaign = state
        .campaign_service
        .create(
            &identity,
            rid,
            NewCampaign {
                tenant_id: identity.tenant_id,
                restaurant_id: rid,
                name: body.name,
                channel,
                subject: body.subject,
                body: body.body,
                tag_filter: body.tag_filter,
                scheduled_at,
            },
        )
        .await
        .map_err(ApiError::from)?;

    Ok((StatusCode::CREATED, Json(CampaignResponse::from(campaign))))
}

/// `GET /api/v1/restaurants/:id/campaigns`
#[tracing::instrument(skip(state), name = "handlers::campaigns::list")]
pub async fn list_campaigns(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    Query(q): Query<CampaignListQuery>,
) -> ApiResult<impl IntoResponse> {
    let status = q.status.as_deref().map(parse_status).transpose()?;

    let page = state
        .campaign_service
        .list(
            &identity,
            RestaurantId::from_uuid(restaurant_id),
            CampaignListParams {
                limit: q.limit.clamp(1, 100),
                cursor: q.cursor.as_deref().and_then(decode_cursor),
                status,
            },
        )
        .await
        .map_err(ApiError::from)?;

    let next_cursor = page.next_cursor.as_ref().map(encode_cursor);
    let items: Vec<CampaignResponse> = page.items.into_iter().map(Into::into).collect();

    Ok(Json(PageResponse::new(items, next_cursor)))
}

/// `GET /api/v1/restaurants/:id/campaigns/:cid`
#[tracing::instrument(skip(state), name = "handlers::campaigns::get")]
pub async fn get_campaign(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((_, cid)): Path<(Uuid, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    let campaign = state
        .campaign_service
        .get(&identity, CampaignId::from_uuid(cid))
        .await
        .map_err(ApiError::from)?;

    Ok(Json(CampaignResponse::from(campaign)))
}

/// `PATCH /api/v1/restaurants/:id/campaigns/:cid`
///
/// Only campaigns in `draft` status can be updated.
#[tracing::instrument(skip(state, body), name = "handlers::campaigns::update")]
pub async fn update_campaign(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((_, cid)): Path<(Uuid, Uuid)>,
    ValidatedJson(body): ValidatedJson<UpdateCampaignBody>,
) -> ApiResult<impl IntoResponse> {
    let scheduled_at = parse_rfc3339_opt(body.scheduled_at.as_deref())?.map(Some); // wrap in outer Some so repo knows to update it

    let campaign = state
        .campaign_service
        .update(
            &identity,
            CampaignId::from_uuid(cid),
            UpdateCampaign {
                name: body.name,
                subject: body.subject.map(Some),
                body: body.body,
                tag_filter: body.tag_filter.map(Some),
                scheduled_at,
            },
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(CampaignResponse::from(campaign)))
}

/// `DELETE /api/v1/restaurants/:id/campaigns/:cid`
///
/// Only `draft` campaigns can be deleted.
#[tracing::instrument(skip(state), name = "handlers::campaigns::delete")]
pub async fn delete_campaign(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((_, cid)): Path<(Uuid, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    state
        .campaign_service
        .delete(&identity, CampaignId::from_uuid(cid))
        .await
        .map_err(ApiError::from)?;

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/restaurants/:id/campaigns/:cid/send`
///
/// Dispatches the campaign immediately.  The send task runs in the background;
/// this endpoint returns `202 Accepted` with the expected recipient count.
///
/// Requirements:
/// - Campaign must be in `draft` status
/// - Email channel requires `APP__EMAIL__SMTP_HOST` to be set
/// - SMS channel returns `501 Not Implemented`
#[tracing::instrument(skip(state), name = "handlers::campaigns::send")]
pub async fn send_campaign(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((_, cid)): Path<(Uuid, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    let result = state
        .campaign_service
        .send(&identity, CampaignId::from_uuid(cid))
        .await
        .map_err(ApiError::from)?;

    Ok((StatusCode::ACCEPTED, Json(SendResponse::from(result))))
}
