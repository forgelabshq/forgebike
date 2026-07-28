//! Restaurant and menu-item handlers.
//!
//! All routes require a valid JWT — apply `require_auth` middleware on the
//! parent router before mounting these handlers.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use forgebike_application::restaurant::commands::{
    CreateMenuItemCommand, CreateRestaurantCommand, UpdateMenuItemCommand, UpdateRestaurantCommand,
};
use forgebike_domain::{
    entities::{auth_identity::AuthIdentity, menu_item::MenuItem, restaurant::Restaurant},
    identifiers::{MenuItemId, RestaurantId},
    pagination::ListParams,
};

use crate::{
    error::{ApiError, ApiResult},
    extractors::ValidatedJson,
    pagination::{decode_cursor, encode_cursor, PageQuery, PageResponse},
    state::AppState,
};

// ---------------------------------------------------------------------------
// Response DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct RestaurantResponse {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub cuisine_type: Option<String>,
    pub address: Option<String>,
    pub phone: Option<String>,
    pub website: Option<String>,
    pub google_place_id: Option<String>,
    pub yelp_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<Restaurant> for RestaurantResponse {
    fn from(r: Restaurant) -> Self {
        Self {
            id: r.id.to_string(),
            name: r.name,
            description: r.description,
            cuisine_type: r.cuisine_type,
            address: r.address,
            phone: r.phone,
            website: r.website,
            google_place_id: r.google_place_id,
            yelp_id: r.yelp_id,
            created_at: r.created_at.to_rfc3339(),
            updated_at: r.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct MenuItemResponse {
    pub id: String,
    pub restaurant_id: String,
    pub name: String,
    pub description: Option<String>,
    pub price_cents: Option<i64>,
    pub category: Option<String>,
    pub is_available: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<MenuItem> for MenuItemResponse {
    fn from(m: MenuItem) -> Self {
        Self {
            id: m.id.to_string(),
            restaurant_id: m.restaurant_id.to_string(),
            name: m.name,
            description: m.description,
            price_cents: m.price_cents,
            category: m.category,
            is_available: m.is_available,
            created_at: m.created_at.to_rfc3339(),
            updated_at: m.updated_at.to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Request DTOs
// ---------------------------------------------------------------------------

#[derive(Deserialize, Validate)]
pub struct CreateRestaurantBody {
    #[validate(length(min = 1, max = 200, message = "Name must be 1–200 characters"))]
    pub name: String,

    #[validate(length(max = 1000, message = "Description must be ≤ 1000 characters"))]
    pub description: Option<String>,

    #[validate(length(max = 100))]
    pub cuisine_type: Option<String>,

    #[validate(length(max = 300))]
    pub address: Option<String>,

    #[validate(length(max = 50))]
    pub phone: Option<String>,

    #[validate(url(message = "website must be a valid URL"))]
    pub website: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct UpdateRestaurantBody {
    #[validate(length(min = 1, max = 200, message = "Name must be 1–200 characters"))]
    pub name: Option<String>,

    #[validate(length(max = 1000))]
    pub description: Option<String>,

    #[validate(length(max = 100))]
    pub cuisine_type: Option<String>,

    #[validate(length(max = 300))]
    pub address: Option<String>,

    #[validate(length(max = 50))]
    pub phone: Option<String>,

    #[validate(url(message = "website must be a valid URL"))]
    pub website: Option<String>,

    #[validate(length(max = 200))]
    pub google_place_id: Option<String>,

    #[validate(length(max = 200))]
    pub yelp_id: Option<String>,
}

#[derive(Deserialize, Validate)]
pub struct CreateMenuItemBody {
    #[validate(length(min = 1, max = 200, message = "Name must be 1–200 characters"))]
    pub name: String,

    #[validate(length(max = 1000))]
    pub description: Option<String>,

    #[validate(range(min = 0, message = "price_cents must be ≥ 0"))]
    pub price_cents: Option<i64>,

    #[validate(length(max = 100))]
    pub category: Option<String>,

    #[serde(default = "default_true")]
    pub is_available: bool,
}

#[derive(Deserialize, Validate)]
pub struct UpdateMenuItemBody {
    #[validate(length(min = 1, max = 200, message = "Name must be 1–200 characters"))]
    pub name: Option<String>,

    #[validate(length(max = 1000))]
    pub description: Option<String>,

    #[validate(range(min = 0, message = "price_cents must be ≥ 0"))]
    pub price_cents: Option<i64>,

    #[validate(length(max = 100))]
    pub category: Option<String>,

    pub is_available: Option<bool>,
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_restaurant_id(raw: Uuid) -> RestaurantId {
    RestaurantId::from_uuid(raw)
}

fn parse_menu_item_id(raw: Uuid) -> MenuItemId {
    MenuItemId::from_uuid(raw)
}

fn page_params(q: &PageQuery) -> ListParams {
    ListParams {
        limit: q.limit.clamp(1, 100),
        cursor: q.cursor.as_deref().and_then(decode_cursor),
    }
}

// ---------------------------------------------------------------------------
// Restaurant handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/restaurants`
#[tracing::instrument(skip(state, body), name = "handlers::restaurants::create")]
pub async fn create_restaurant(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    ValidatedJson(body): ValidatedJson<CreateRestaurantBody>,
) -> ApiResult<impl IntoResponse> {
    let restaurant = state
        .restaurant_service
        .create_restaurant(
            &identity,
            CreateRestaurantCommand {
                name: body.name,
                description: body.description,
                cuisine_type: body.cuisine_type,
                address: body.address,
                phone: body.phone,
                website: body.website,
            },
        )
        .await
        .map_err(ApiError::from)?;

    Ok((
        StatusCode::CREATED,
        Json(RestaurantResponse::from(restaurant)),
    ))
}

/// `GET /api/v1/restaurants`
#[tracing::instrument(skip(state), name = "handlers::restaurants::list")]
pub async fn list_restaurants(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Query(q): Query<PageQuery>,
) -> ApiResult<impl IntoResponse> {
    let page = state
        .restaurant_service
        .list_restaurants(&identity, page_params(&q))
        .await
        .map_err(ApiError::from)?;

    let next_cursor = page.next_cursor.as_ref().map(encode_cursor);
    let items: Vec<RestaurantResponse> = page.items.into_iter().map(Into::into).collect();

    Ok(Json(PageResponse::new(items, next_cursor)))
}

/// `GET /api/v1/restaurants/:id`
#[tracing::instrument(skip(state), name = "handlers::restaurants::get")]
pub async fn get_restaurant(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    let restaurant = state
        .restaurant_service
        .get_restaurant(&identity, parse_restaurant_id(id))
        .await
        .map_err(ApiError::from)?;

    Ok(Json(RestaurantResponse::from(restaurant)))
}

/// `PATCH /api/v1/restaurants/:id`
#[tracing::instrument(skip(state, body), name = "handlers::restaurants::update")]
pub async fn update_restaurant(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<UpdateRestaurantBody>,
) -> ApiResult<impl IntoResponse> {
    let restaurant = state
        .restaurant_service
        .update_restaurant(
            &identity,
            parse_restaurant_id(id),
            UpdateRestaurantCommand {
                name: body.name,
                description: body.description,
                cuisine_type: body.cuisine_type,
                address: body.address,
                phone: body.phone,
                website: body.website,
                google_place_id: body.google_place_id,
                yelp_id: body.yelp_id,
            },
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(RestaurantResponse::from(restaurant)))
}

/// `DELETE /api/v1/restaurants/:id`
#[tracing::instrument(skip(state), name = "handlers::restaurants::delete")]
pub async fn delete_restaurant(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    state
        .restaurant_service
        .delete_restaurant(&identity, parse_restaurant_id(id))
        .await
        .map_err(ApiError::from)?;

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Menu item handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/restaurants/:id/menu`
#[tracing::instrument(skip(state, body), name = "handlers::restaurants::create_item")]
pub async fn create_menu_item(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<CreateMenuItemBody>,
) -> ApiResult<impl IntoResponse> {
    let item = state
        .restaurant_service
        .create_menu_item(
            &identity,
            parse_restaurant_id(restaurant_id),
            CreateMenuItemCommand {
                name: body.name,
                description: body.description,
                price_cents: body.price_cents,
                category: body.category,
                is_available: body.is_available,
            },
        )
        .await
        .map_err(ApiError::from)?;

    Ok((StatusCode::CREATED, Json(MenuItemResponse::from(item))))
}

/// `GET /api/v1/restaurants/:id/menu`
#[tracing::instrument(skip(state), name = "handlers::restaurants::list_menu")]
pub async fn list_menu_items(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    Query(q): Query<PageQuery>,
) -> ApiResult<impl IntoResponse> {
    let page = state
        .restaurant_service
        .list_menu_items(
            &identity,
            parse_restaurant_id(restaurant_id),
            page_params(&q),
        )
        .await
        .map_err(ApiError::from)?;

    let next_cursor = page.next_cursor.as_ref().map(encode_cursor);
    let items: Vec<MenuItemResponse> = page.items.into_iter().map(Into::into).collect();

    Ok(Json(PageResponse::new(items, next_cursor)))
}

/// `PATCH /api/v1/restaurants/:restaurant_id/menu/:item_id`
#[tracing::instrument(skip(state, body), name = "handlers::restaurants::update_item")]
pub async fn update_menu_item(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((restaurant_id, item_id)): Path<(Uuid, Uuid)>,
    ValidatedJson(body): ValidatedJson<UpdateMenuItemBody>,
) -> ApiResult<impl IntoResponse> {
    let item = state
        .restaurant_service
        .update_menu_item(
            &identity,
            parse_restaurant_id(restaurant_id),
            parse_menu_item_id(item_id),
            UpdateMenuItemCommand {
                name: body.name,
                description: body.description,
                price_cents: body.price_cents,
                category: body.category,
                is_available: body.is_available,
            },
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(MenuItemResponse::from(item)))
}

/// `DELETE /api/v1/restaurants/:restaurant_id/menu/:item_id`
#[tracing::instrument(skip(state), name = "handlers::restaurants::delete_item")]
pub async fn delete_menu_item(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((restaurant_id, item_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    state
        .restaurant_service
        .delete_menu_item(
            &identity,
            parse_restaurant_id(restaurant_id),
            parse_menu_item_id(item_id),
        )
        .await
        .map_err(ApiError::from)?;

    Ok(StatusCode::NO_CONTENT)
}
