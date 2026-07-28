//! JWT authentication middleware.
//!
//! Apply with `axum::middleware::from_fn_with_state(state, require_auth)` to
//! any router or individual route that requires a valid session.
//!
//! On success, the decoded [`AuthIdentity`] is inserted into the request
//! extensions and can be extracted by downstream handlers using the
//! [`crate::extractors::role`] extractors.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, DecodingKey, Validation};

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

fn decode_token(token: &str, secret: &str) -> Result<AuthIdentity, ApiError> {
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
