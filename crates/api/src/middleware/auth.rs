//! JWT authentication middleware.
//!
//! Apply with `axum::middleware::from_fn_with_state(state, require_auth)` to
//! any router or individual route that requires a valid session.
//!
//! On success, the decoded [`AuthIdentity`] is inserted into the request
//! extensions and can be extracted by downstream handlers using the
//! [`crate::extractors::role`] extractors.

use axum::{
    extract::{Query, Request, State},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::Deserialize;

use forgebike_application::auth::claims::AccessTokenClaims;
use forgebike_domain::{
    entities::{auth_identity::AuthIdentity, user::UserRole},
    identifiers::{TenantId, UserId},
};

use crate::{error::ApiError, state::AppState};

/// Middleware that validates a `Bearer` token and injects [`AuthIdentity`]
/// into the request extensions.
///
/// Returns `401 Unauthorised` if the token is absent, malformed, or expired.
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = extract_bearer(&request)?;
    let identity = decode_token(token, &state.config.jwt.secret)?;
    request.extensions_mut().insert(identity);
    Ok(next.run(request).await)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn extract_bearer(request: &Request) -> Result<&str, ApiError> {
    request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(ApiError::unauthorised)
}

// ---------------------------------------------------------------------------
// WebSocket query-param auth
// ---------------------------------------------------------------------------

/// Query parameters for WebSocket endpoints that carry the JWT in the URL.
#[derive(Debug, Deserialize)]
pub struct WsTokenQuery {
    /// Access JWT — browsers cannot set `Authorization` headers on WS
    /// handshakes, so the token is passed as a query parameter instead.
    token: Option<String>,
}

/// Middleware for WebSocket routes: validates `?token=<jwt>` and injects
/// [`AuthIdentity`] into request extensions before the handler runs.
///
/// By running auth in middleware the check happens **before** axum's
/// [`axum::extract::ws::WebSocketUpgrade`] extractor, so unauthenticated or
/// invalidly-authenticated plain-HTTP requests receive `401` rather than
/// `400 Bad Request` (which is what `WebSocketUpgrade` would return for a
/// non-upgrade request if the handler ran first).
pub async fn require_ws_auth(
    State(state): State<AppState>,
    Query(q): Query<WsTokenQuery>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = q.token.as_deref().unwrap_or("");
    let identity = decode_token(token, &state.config.jwt.secret)?;
    request.extensions_mut().insert(identity);
    Ok(next.run(request).await)
}

// ---------------------------------------------------------------------------
// Token decode helper (shared by both auth middleware implementations)
// ---------------------------------------------------------------------------

/// Validate a raw JWT string and return the decoded identity.
pub(crate) fn decode_token(token: &str, secret: &str) -> Result<AuthIdentity, ApiError> {
    let mut validation = Validation::default();
    validation.validate_exp = true;

    let data = decode::<AccessTokenClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| ApiError::unauthorised())?;

    let claims = data.claims;

    let user_id = claims
        .sub
        .parse::<UserId>()
        .map_err(|_| ApiError::unauthorised())?;

    let tenant_id = claims
        .tenant_id
        .parse::<TenantId>()
        .map_err(|_| ApiError::unauthorised())?;

    let role = claims
        .role
        .parse::<UserRole>()
        .map_err(|_| ApiError::unauthorised())?;

    Ok(AuthIdentity {
        user_id,
        tenant_id,
        role,
    })
}
