//! JWT payload (claims) for access tokens.
//!
//! Defined here so both the application layer (encoding) and the API layer
//! (decoding in middleware) share the same struct without introducing a
//! circular dependency.

use serde::{Deserialize, Serialize};

/// The payload encoded inside every access token (HS256).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccessTokenClaims {
    /// Subject — the `UserId` serialised as a UUID string.
    pub sub: String,
    /// The `TenantId` serialised as a UUID string.
    pub tenant_id: String,
    /// The user's role: `"owner"`, `"manager"`, or `"viewer"`.
    pub role: String,
    /// Unix timestamp — token not valid before this time.
    pub iat: u64,
    /// Unix timestamp — token expires at this time.
    pub exp: u64,
}
