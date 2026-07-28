//! Command and response types for the auth use cases.
//!
//! Commands are plain data structs — no validation logic lives here.
//! Validation is done in the API layer before constructing these types.

// ---------------------------------------------------------------------------
// Inbound commands
// ---------------------------------------------------------------------------

pub struct RegisterCommand {
    /// The name of the restaurant business (becomes the tenant name).
    pub business_name: String,
    pub email: String,
    pub password: String,
}

pub struct LoginCommand {
    pub email: String,
    pub password: String,
}

pub struct RefreshCommand {
    /// The raw refresh token as issued to the client.
    pub refresh_token: String,
}

pub struct LogoutCommand {
    /// The raw refresh token to revoke.
    pub refresh_token: String,
}

// ---------------------------------------------------------------------------
// Outbound response
// ---------------------------------------------------------------------------

/// Returned after a successful register, login, or refresh.
#[derive(Debug)]
pub struct AuthTokenPair {
    pub access_token: String,
    pub refresh_token: String,
    /// Seconds until the access token expires.
    pub expires_in: u64,
}
