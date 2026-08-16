//! Marketing content handlers — generate, list, stream, update, delete.

use std::{convert::Infallible, sync::Arc};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    Extension, Json,
};
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

use forgebike_application::content::commands::{
    ContentListQuery, GenerateContentCommand, UpdateContentCommand,
};
use forgebike_domain::{
    entities::{
        auth_identity::AuthIdentity,
        content_piece::{ContentPiece, ContentStatus, ContentType},
    },
    identifiers::{ContentPieceId, RestaurantId},
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
pub struct ContentResponse {
    pub id: String,
    pub content_type: String,
    pub title: Option<String>,
    pub body: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<ContentPiece> for ContentResponse {
    fn from(p: ContentPiece) -> Self {
        Self {
            id: p.id.to_string(),
            content_type: p.content_type.to_string(),
            title: p.title,
            body: p.body,
            status: p.status.to_string(),
            created_at: p.created_at.to_rfc3339(),
            updated_at: p.updated_at.to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Request DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, validator::Validate)]
pub struct GenerateBody {
    /// `social_post` | `email` | `menu_description` | `blog_intro`
    #[validate(length(min = 1))]
    pub content_type: String,

    #[validate(length(max = 200))]
    pub topic: Option<String>,

    #[validate(length(max = 100))]
    pub tone: Option<String>,
}

#[derive(Deserialize, validator::Validate)]
pub struct UpdateBody {
    #[validate(length(max = 300))]
    pub title: Option<String>,

    #[validate(length(min = 1))]
    pub body: Option<String>,

    /// `draft` | `approved` | `published`
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ContentQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub cursor: Option<String>,
    pub status: Option<String>,
    pub content_type: Option<String>,
}

fn default_limit() -> i64 {
    20
}

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

fn parse_content_type(s: &str) -> ApiResult<ContentType> {
    s.parse::<ContentType>().map_err(|_| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("unknown content_type: {s:?}. Use: social_post, email, menu_description, blog_intro"),
        )
    })
}

fn parse_content_status(s: &str) -> ApiResult<ContentStatus> {
    s.parse::<ContentStatus>().map_err(|_| {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("unknown status: {s:?}. Use: draft, approved, published"),
        )
    })
}

fn build_generate_cmd(body: &GenerateBody) -> ApiResult<GenerateContentCommand> {
    Ok(GenerateContentCommand {
        content_type: parse_content_type(&body.content_type)?,
        topic: body.topic.clone(),
        tone: body.tone.clone(),
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/restaurants/:id/content/generate`
///
/// Generates marketing content synchronously and stores it as a `draft`.
/// Returns `503` when the `OpenAI` API key is not configured.
#[tracing::instrument(skip(state, body), name = "handlers::content::generate")]
pub async fn generate(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<GenerateBody>,
) -> ApiResult<impl IntoResponse> {
    state
        .billing_service
        .check_ai_budget(identity.tenant_id)
        .await
        .map_err(ApiError::from)?;

    let cmd = build_generate_cmd(&body)?;
    let piece = state
        .content_service
        .generate(&identity, RestaurantId::from_uuid(restaurant_id), cmd)
        .await
        .map_err(ApiError::from)?;

    Ok((StatusCode::CREATED, Json(ContentResponse::from(piece))))
}

/// `GET /api/v1/restaurants/:id/content/stream`
///
/// Opens a Server-Sent Events stream and generates content token by token.
/// The final event has `data: __done__:<content_id>` so the client can fetch
/// the saved piece immediately after the stream closes.
///
/// Returns `503` when the `OpenAI` API key is not configured.
#[tracing::instrument(skip(state), name = "handlers::content::stream")]
pub async fn stream(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    Query(q): Query<GenerateBody>,
) -> ApiResult<Sse<impl futures::Stream<Item = Result<Event, Infallible>>>> {
    state
        .billing_service
        .check_ai_budget(identity.tenant_id)
        .await
        .map_err(ApiError::from)?;

    let cmd = build_generate_cmd(&q)?;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let tx_done = tx.clone();

    let svc = Arc::clone(&state.content_service);
    let identity = identity.clone();
    let rid = RestaurantId::from_uuid(restaurant_id);

    let on_chunk: Arc<dyn Fn(String) + Send + Sync> = Arc::new(move |chunk| {
        let _ = tx.send(chunk);
    });

    tokio::spawn(async move {
        let signal = match svc.stream_generate(&identity, rid, cmd, on_chunk).await {
            Ok(piece) => format!("__done__:{}", piece.id),
            Err(e) => format!("__error__:{e}"),
        };
        let _ = tx_done.send(signal);
    });

    let stream = UnboundedReceiverStream::new(rx)
        .map(|chunk: String| Ok::<_, Infallible>(Event::default().data(chunk)));

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// `GET /api/v1/restaurants/:id/content`
#[tracing::instrument(skip(state), name = "handlers::content::list")]
pub async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    Query(q): Query<ContentQuery>,
) -> ApiResult<impl IntoResponse> {
    let status = q.status.as_deref().map(parse_content_status).transpose()?;

    let content_type = q
        .content_type
        .as_deref()
        .map(parse_content_type)
        .transpose()?;

    let page = state
        .content_service
        .list(
            &identity,
            RestaurantId::from_uuid(restaurant_id),
            ContentListQuery {
                limit: q.limit.clamp(1, 100),
                cursor: q.cursor.as_deref().and_then(decode_cursor),
                status,
                content_type,
            },
        )
        .await
        .map_err(ApiError::from)?;

    let next_cursor = page.next_cursor.as_ref().map(encode_cursor);
    let items: Vec<ContentResponse> = page.items.into_iter().map(Into::into).collect();

    Ok(Json(PageResponse::new(items, next_cursor)))
}

/// `GET /api/v1/restaurants/:id/content/:cid`
#[tracing::instrument(skip(state), name = "handlers::content::get")]
pub async fn get_piece(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((restaurant_id, cid)): Path<(Uuid, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    let piece = state
        .content_service
        .get(
            &identity,
            RestaurantId::from_uuid(restaurant_id),
            ContentPieceId::from_uuid(cid),
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(ContentResponse::from(piece)))
}

/// `PATCH /api/v1/restaurants/:id/content/:cid`
///
/// Partial update — omitted fields are left unchanged.  Use `status` to move
/// a piece through `draft → approved → published`.
#[tracing::instrument(skip(state, body), name = "handlers::content::update")]
pub async fn update(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((restaurant_id, cid)): Path<(Uuid, Uuid)>,
    ValidatedJson(body): ValidatedJson<UpdateBody>,
) -> ApiResult<impl IntoResponse> {
    let status = body
        .status
        .as_deref()
        .map(parse_content_status)
        .transpose()?;

    let piece = state
        .content_service
        .update(
            &identity,
            RestaurantId::from_uuid(restaurant_id),
            ContentPieceId::from_uuid(cid),
            UpdateContentCommand {
                title: body.title,
                body: body.body,
                status,
            },
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(ContentResponse::from(piece)))
}

/// `DELETE /api/v1/restaurants/:id/content/:cid`
#[tracing::instrument(skip(state), name = "handlers::content::delete")]
pub async fn delete(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((restaurant_id, cid)): Path<(Uuid, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    state
        .content_service
        .delete(
            &identity,
            RestaurantId::from_uuid(restaurant_id),
            ContentPieceId::from_uuid(cid),
        )
        .await
        .map_err(ApiError::from)?;

    Ok(StatusCode::NO_CONTENT)
}
