//! [`AuthService`] — orchestrates all authentication use cases.

use std::sync::Arc;

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use jsonwebtoken::{encode, EncodingKey, Header};
use uuid::Uuid;

use forgebike_config::JwtConfig;
use forgebike_domain::{
    entities::user::User,
    identifiers::UserId,
    ports::{
        tenant_repository::{NewTenant, TenantRepository},
        token_store::{StoredTokenData, TokenStore},
        user_repository::{NewUser, UserRepository},
    },
    DomainError,
};

use super::{
    claims::AccessTokenClaims,
    commands::{AuthTokenPair, LoginCommand, LogoutCommand, RefreshCommand, RegisterCommand},
    error::AuthError,
};

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

pub struct AuthService {
    users: Arc<dyn UserRepository>,
    tenants: Arc<dyn TenantRepository>,
    token_store: Arc<dyn TokenStore>,
    jwt_config: JwtConfig,
}

impl AuthService {
    pub fn new(
        users: Arc<dyn UserRepository>,
        tenants: Arc<dyn TenantRepository>,
        token_store: Arc<dyn TokenStore>,
        jwt_config: JwtConfig,
    ) -> Self {
        Self {
            users,
            tenants,
            token_store,
            jwt_config,
        }
    }

    // -----------------------------------------------------------------------
    // Use cases
    // -----------------------------------------------------------------------

    /// Create a new tenant and its first owner user, returning a token pair.
    ///
    /// Each call always creates a **fresh** tenant, so the same email address
    /// can register multiple times if they run separate restaurant businesses.
    pub async fn register(&self, cmd: RegisterCommand) -> Result<AuthTokenPair, AuthError> {
        let password_hash = hash_password(&cmd.password)?;

        let tenant = self
            .tenants
            .create(NewTenant {
                name: cmd.business_name,
            })
            .await?;

        let user = self
            .users
            .create(NewUser {
                tenant_id: tenant.id,
                email: cmd.email,
                password_hash,
                role: forgebike_domain::entities::user::UserRole::Owner,
            })
            .await?;

        self.issue_pair(&user).await
    }

    /// Verify credentials and return a token pair.
    pub async fn login(&self, cmd: LoginCommand) -> Result<AuthTokenPair, AuthError> {
        let user = self
            .users
            .find_by_email(&cmd.email)
            .await?
            .ok_or(AuthError::InvalidCredentials)?;

        verify_password(&cmd.password, &user.password_hash)?;

        self.issue_pair(&user).await
    }

    /// Validate a refresh token, rotate it, and return a new token pair.
    pub async fn refresh(&self, cmd: RefreshCommand) -> Result<AuthTokenPair, AuthError> {
        let data = self
            .token_store
            .get(&cmd.refresh_token)
            .await?
            .ok_or(AuthError::InvalidRefreshToken)?;

        // Rotate: revoke the old token before issuing the new one.
        self.token_store.revoke(&cmd.refresh_token).await?;

        let user = self
            .users
            .find_by_id(data.user_id)
            .await?
            .ok_or(AuthError::InvalidRefreshToken)?;

        self.issue_pair(&user).await
    }

    /// Revoke a refresh token, ending the session.
    pub async fn logout(&self, cmd: LogoutCommand) -> Result<(), AuthError> {
        // Best-effort: if the token is already gone (expired or double-logout)
        // we treat it as success to avoid leaking session state to the caller.
        self.token_store.revoke(&cmd.refresh_token).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    async fn issue_pair(&self, user: &User) -> Result<AuthTokenPair, AuthError> {
        let access_token = self.encode_access_token(user)?;
        let refresh_token = Uuid::new_v4().to_string();

        self.token_store
            .store(
                &refresh_token,
                StoredTokenData {
                    user_id: user.id,
                    tenant_id: user.tenant_id,
                    role: user.role.clone(),
                },
                self.jwt_config.refresh_token_expiry_secs,
            )
            .await?;

        Ok(AuthTokenPair {
            access_token,
            refresh_token,
            expires_in: self.jwt_config.access_token_expiry_secs,
        })
    }

    fn encode_access_token(&self, user: &User) -> Result<String, AuthError> {
        #[allow(clippy::cast_sign_loss)]
        let now = chrono::Utc::now().timestamp() as u64;

        let claims = AccessTokenClaims {
            sub: user.id.to_string(),
            tenant_id: user.tenant_id.to_string(),
            role: user.role.to_string(),
            iat: now,
            exp: now + self.jwt_config.access_token_expiry_secs,
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_config.secret.as_bytes()),
        )
        .map_err(|e| AuthError::Domain(DomainError::Internal(e.to_string())))
    }
}

// ---------------------------------------------------------------------------
// Password helpers (free functions, not tied to the service struct)
// ---------------------------------------------------------------------------

fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Domain(DomainError::Internal(e.to_string())))
}

fn verify_password(password: &str, hash: &str) -> Result<(), AuthError> {
    let parsed = PasswordHash::new(hash).map_err(|_| AuthError::InvalidCredentials)?;
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AuthError::InvalidCredentials)
}

// Silence the unused import — UserId is needed for the type bound on find_by_id
// but Rust doesn't see it as "used" through the trait call.
fn _use_user_id(_: UserId) {}
