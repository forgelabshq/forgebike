//! Customer-contact management handlers.
//!
//! ## Endpoints
//! | Method | Path | Description |
//! |---|---|---|
//! | `POST`   | `/api/v1/restaurants/:id/contacts`          | Create a contact |
//! | `GET`    | `/api/v1/restaurants/:id/contacts`          | List contacts (paginated, tag-filterable) |
//! | `GET`    | `/api/v1/restaurants/:id/contacts/:cid`     | Get a contact |
//! | `PATCH`  | `/api/v1/restaurants/:id/contacts/:cid`     | Update a contact |
//! | `DELETE` | `/api/v1/restaurants/:id/contacts/:cid`     | Delete a contact |
//! | `POST`   | `/api/v1/restaurants/:id/contacts/import`   | Bulk-import contacts from a JSON array |

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use forgebike_domain::{
    entities::{auth_identity::AuthIdentity, customer_contact::CustomerContact},
    identifiers::{CustomerContactId, RestaurantId},
    ports::customer_contact_repository::{ContactListParams, NewContact, UpdateContact},
};

use crate::{
    error::{ApiError, ApiResult},
    extractors::ValidatedJson,
    pagination::{decode_cursor, encode_cursor, PageResponse},
    state::AppState,
};

// ---------------------------------------------------------------------------
// Response DTO
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ContactResponse {
    pub id: String,
    pub restaurant_id: String,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub tags: Vec<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<CustomerContact> for ContactResponse {
    fn from(c: CustomerContact) -> Self {
        Self {
            id: c.id.to_string(),
            restaurant_id: c.restaurant_id.to_string(),
            name: c.name,
            email: c.email,
            phone: c.phone,
            tags: c.tags,
            notes: c.notes,
            created_at: c.created_at.to_rfc3339(),
            updated_at: c.updated_at.to_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Request DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, validator::Validate)]
pub struct CreateContactBody {
    #[validate(length(min = 1, max = 200))]
    pub name: String,

    #[validate(email)]
    pub email: Option<String>,

    #[validate(length(max = 30))]
    pub phone: Option<String>,

    #[serde(default)]
    pub tags: Vec<String>,

    #[validate(length(max = 1000))]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, validator::Validate)]
pub struct UpdateContactBody {
    #[validate(length(min = 1, max = 200))]
    pub name: Option<String>,

    // `null` in JSON clears the value; omitting the key leaves it unchanged.
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    pub email: Option<Option<String>>,

    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    pub phone: Option<Option<String>>,

    pub tags: Option<Vec<String>>,

    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    pub notes: Option<Option<String>>,
}

/// Deserialise a field that can be absent, `null`, or a string.
/// - Absent → `None`  (outer None → do not update)
/// - `null` → `Some(None)` (clear the field)
/// - `"value"` → `Some(Some("value"))` (set a new value)
#[allow(clippy::option_option)]
fn deserialize_nullable_string<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let v = serde_json::Value::deserialize(de).map_err(Error::custom)?;
    match v {
        serde_json::Value::Null => Ok(Some(None)),
        serde_json::Value::String(s) => Ok(Some(Some(s))),
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) => {
            Err(Error::custom("expected a string or null"))
        }
        _ => Ok(None), // missing field (Unit) handled by serde default
    }
}

#[derive(Debug, Deserialize)]
pub struct ContactListQuery {
    #[serde(default = "default_limit")]
    pub limit: i64,
    pub cursor: Option<String>,
    pub tag: Option<String>,
}

fn default_limit() -> i64 {
    20
}

/// A single contact in a bulk-import request.
#[derive(Debug, Deserialize, Serialize, validator::Validate)]
pub struct ImportContactItem {
    #[validate(length(min = 1, max = 200))]
    pub name: String,

    #[validate(email)]
    pub email: Option<String>,

    #[validate(length(max = 30))]
    pub phone: Option<String>,

    #[serde(default)]
    pub tags: Vec<String>,

    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, validator::Validate)]
pub struct ImportBody {
    #[validate(length(min = 1))]
    pub contacts: Vec<ImportContactItem>,
}

#[derive(Serialize)]
pub struct ImportResponse {
    pub imported: usize,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/restaurants/:id/contacts`
#[tracing::instrument(skip(state, body), name = "handlers::contacts::create")]
pub async fn create_contact(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<CreateContactBody>,
) -> ApiResult<impl IntoResponse> {
    let rid = RestaurantId::from_uuid(restaurant_id);
    let contact = state
        .contact_service
        .create(
            &identity,
            rid,
            NewContact {
                tenant_id: identity.tenant_id,
                restaurant_id: rid,
                name: body.name,
                email: body.email,
                phone: body.phone,
                tags: body.tags,
                notes: body.notes,
            },
        )
        .await
        .map_err(ApiError::from)?;

    Ok((StatusCode::CREATED, Json(ContactResponse::from(contact))))
}

/// `GET /api/v1/restaurants/:id/contacts`
#[tracing::instrument(skip(state), name = "handlers::contacts::list")]
pub async fn list_contacts(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    Query(q): Query<ContactListQuery>,
) -> ApiResult<impl IntoResponse> {
    let page = state
        .contact_service
        .list(
            &identity,
            RestaurantId::from_uuid(restaurant_id),
            ContactListParams {
                limit: q.limit.clamp(1, 100),
                cursor: q.cursor.as_deref().and_then(decode_cursor),
                tag: q.tag,
            },
        )
        .await
        .map_err(ApiError::from)?;

    let next_cursor = page.next_cursor.as_ref().map(encode_cursor);
    let items: Vec<ContactResponse> = page.items.into_iter().map(Into::into).collect();

    Ok(Json(PageResponse::new(items, next_cursor)))
}

/// `GET /api/v1/restaurants/:id/contacts/:cid`
#[tracing::instrument(skip(state), name = "handlers::contacts::get")]
pub async fn get_contact(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((_, cid)): Path<(Uuid, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    let contact = state
        .contact_service
        .get(&identity, CustomerContactId::from_uuid(cid))
        .await
        .map_err(ApiError::from)?;

    Ok(Json(ContactResponse::from(contact)))
}

/// `PATCH /api/v1/restaurants/:id/contacts/:cid`
#[tracing::instrument(skip(state, body), name = "handlers::contacts::update")]
pub async fn update_contact(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((_, cid)): Path<(Uuid, Uuid)>,
    ValidatedJson(body): ValidatedJson<UpdateContactBody>,
) -> ApiResult<impl IntoResponse> {
    let contact = state
        .contact_service
        .update(
            &identity,
            CustomerContactId::from_uuid(cid),
            UpdateContact {
                name: body.name,
                email: body.email,
                phone: body.phone,
                tags: body.tags,
                notes: body.notes,
            },
        )
        .await
        .map_err(ApiError::from)?;

    Ok(Json(ContactResponse::from(contact)))
}

/// `DELETE /api/v1/restaurants/:id/contacts/:cid`
#[tracing::instrument(skip(state), name = "handlers::contacts::delete")]
pub async fn delete_contact(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path((_, cid)): Path<(Uuid, Uuid)>,
) -> ApiResult<impl IntoResponse> {
    state
        .contact_service
        .delete(&identity, CustomerContactId::from_uuid(cid))
        .await
        .map_err(ApiError::from)?;

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/restaurants/:id/contacts/import`
///
/// Accepts a JSON array of contacts and bulk-inserts them.
/// Duplicate emails (per restaurant) are silently skipped.
#[tracing::instrument(skip(state, body), name = "handlers::contacts::import")]
pub async fn import_contacts(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<ImportBody>,
) -> ApiResult<impl IntoResponse> {
    let rid = RestaurantId::from_uuid(restaurant_id);

    let new_contacts: Vec<NewContact> = body
        .contacts
        .into_iter()
        .map(|item| NewContact {
            tenant_id: identity.tenant_id,
            restaurant_id: rid,
            name: item.name,
            email: item.email,
            phone: item.phone,
            tags: item.tags,
            notes: item.notes,
        })
        .collect();

    let imported = state
        .contact_service
        .bulk_import(&identity, rid, new_contacts)
        .await
        .map_err(ApiError::from)?;

    Ok((StatusCode::CREATED, Json(ImportResponse { imported })))
}
