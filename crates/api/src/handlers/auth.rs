//! Auth endpoint handlers — thin translators between HTTP and the application layer.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};
use validator::Validate;

use forgebike_application::auth::commands::{
    LoginCommand, LogoutCommand, RefreshCommand, RegisterCommand,
};
use forgebike_domain::entities::auth_identity::AuthIdentity;

use crate::{
    error::{ApiError, ApiResult},
    extractors::ValidatedJson,
    state::AppState,
};

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

#[derive(Deserialize, Validate)]
pub struct RegisterBody {
    #[validate(length(min = 1, max = 200, message = "Business name must be 1–200 characters"))]
    pub business_name: String,

    #[validate(email(message = "Must be a valid email address"))]
    pub email: String,

    #[validate(length(min = 8, message = "Password must be at least 8 characters"))]
    pub password: String,
}

#[derive(Deserialize, Validate)]
pub struct LoginBody {
    #[validate(email(message = "Must be a valid email address"))]
    pub email: String,

    #[validate(length(min = 1, message = "Password is required"))]
    pub password: String,
}

#[derive(Deserialize, Validate)]
pub struct RefreshBody {
    #[validate(length(min = 1, message = "refresh_token is required"))]
    pub refresh_token: String,
}

#[derive(Deserialize, Validate)]
pub struct LogoutBody {
    #[validate(length(min = 1, message = "refresh_token is required"))]
    pub refresh_token: String,
}

// ---------------------------------------------------------------------------
// Response bodies
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub token_type: &'static str,
}

#[derive(Serialize)]
pub struct MeResponse {
    pub user_id: String,
    pub tenant_id: String,
    pub role: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `POST /api/v1/auth/register`
#[tracing::instrument(skip(state, body), name = "handlers::auth::register")]
pub async fn register(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<RegisterBody>,
) -> ApiResult<impl IntoResponse> {
    let pair = state
        .auth_service
        .register(RegisterCommand {
            business_name: body.business_name,
            email: body.email,
            password: body.password,
        })
        .await
        .map_err(ApiError::from)?;

    Ok((
        StatusCode::CREATED,
        Json(TokenResponse {
            access_token: pair.access_token,
            refresh_token: pair.refresh_token,
            expires_in: pair.expires_in,
            token_type: "Bearer",
        }),
    ))
}

/// `POST /api/v1/auth/login`
#[tracing::instrument(skip(state, body), name = "handlers::auth::login")]
pub async fn login(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<LoginBody>,
) -> ApiResult<impl IntoResponse> {
    let pair = state
        .auth_service
        .login(LoginCommand {
            email: body.email,
            password: body.password,
        })
        .await
        .map_err(ApiError::from)?;

    Ok(Json(TokenResponse {
        access_token: pair.access_token,
        refresh_token: pair.refresh_token,
        expires_in: pair.expires_in,
        token_type: "Bearer",
    }))
}

/// `POST /api/v1/auth/refresh`
#[tracing::instrument(skip(state, body), name = "handlers::auth::refresh")]
pub async fn refresh(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<RefreshBody>,
) -> ApiResult<impl IntoResponse> {
    let pair = state
        .auth_service
        .refresh(RefreshCommand {
            refresh_token: body.refresh_token,
        })
        .await
        .map_err(ApiError::from)?;

    Ok(Json(TokenResponse {
        access_token: pair.access_token,
        refresh_token: pair.refresh_token,
        expires_in: pair.expires_in,
        token_type: "Bearer",
    }))
}

/// `POST /api/v1/auth/logout`
#[tracing::instrument(skip(state, body), name = "handlers::auth::logout")]
pub async fn logout(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<LogoutBody>,
) -> ApiResult<impl IntoResponse> {
    state
        .auth_service
        .logout(LogoutCommand {
            refresh_token: body.refresh_token,
        })
        .await
        .map_err(ApiError::from)?;

    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/auth/me` — protected, requires valid JWT.
///
/// Uses axum's built-in `Extension` extractor; the auth middleware must have
/// inserted an [`AuthIdentity`] into request extensions before this handler runs.
#[tracing::instrument(skip(_state), name = "handlers::auth::me")]
pub async fn me(
    State(_state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
) -> ApiResult<impl IntoResponse> {
    Ok(Json(MeResponse {
        user_id: identity.user_id.to_string(),
        tenant_id: identity.tenant_id.to_string(),
        role: identity.role.to_string(),
    }))
}
