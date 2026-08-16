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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use async_trait::async_trait;
    use chrono::Utc;

    use forgebike_config::JwtConfig;
    use forgebike_domain::{
        entities::{
            tenant::{PlanTier, Tenant},
            user::{User, UserRole},
        },
        identifiers::{TenantId, UserId},
        ports::{
            tenant_repository::{NewTenant, TenantRepository},
            token_store::{StoredTokenData, TokenStore},
            user_repository::{NewUser, UserRepository},
        },
        DomainError,
    };

    use super::{
        super::{commands::*, error::AuthError},
        AuthService,
    };

    // -----------------------------------------------------------------------
    // In-memory mock implementations
    // -----------------------------------------------------------------------

    struct MockUsers(Mutex<Vec<User>>);

    impl MockUsers {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(Mutex::new(vec![])))
        }
    }

    #[async_trait]
    impl UserRepository for MockUsers {
        async fn create(&self, n: NewUser) -> Result<User, DomainError> {
            let user = User {
                id: UserId::new(),
                tenant_id: n.tenant_id,
                email: n.email,
                password_hash: n.password_hash,
                role: n.role,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.0.lock().unwrap().push(user.clone());
            Ok(user)
        }

        async fn find_by_email(&self, email: &str) -> Result<Option<User>, DomainError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .find(|u| u.email == email)
                .cloned())
        }

        async fn find_by_id(&self, id: UserId) -> Result<Option<User>, DomainError> {
            Ok(self.0.lock().unwrap().iter().find(|u| u.id == id).cloned())
        }
    }

    struct MockTenants(Mutex<Vec<Tenant>>);

    impl MockTenants {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(Mutex::new(vec![])))
        }
    }

    #[async_trait]
    impl TenantRepository for MockTenants {
        async fn create(&self, n: NewTenant) -> Result<Tenant, DomainError> {
            let tenant = Tenant {
                id: TenantId::new(),
                name: n.name,
                plan_tier: PlanTier::default(),
                stripe_customer_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.0.lock().unwrap().push(tenant.clone());
            Ok(tenant)
        }

        async fn find_by_id(&self, id: TenantId) -> Result<Option<Tenant>, DomainError> {
            Ok(self.0.lock().unwrap().iter().find(|t| t.id == id).cloned())
        }

        async fn update_plan(
            &self,
            id: TenantId,
            plan: PlanTier,
            _stripe_customer_id: Option<&str>,
        ) -> Result<Tenant, DomainError> {
            let mut lock = self.0.lock().unwrap();
            let t = lock
                .iter_mut()
                .find(|t| t.id == id)
                .ok_or_else(|| DomainError::NotFound(format!("Tenant {id} not found")))?;
            t.plan_tier = plan;
            Ok(t.clone())
        }

        async fn find_by_stripe_customer_id(
            &self,
            _stripe_customer_id: &str,
        ) -> Result<Option<Tenant>, DomainError> {
            Ok(None)
        }

        async fn list_all(&self) -> Result<Vec<Tenant>, DomainError> {
            Ok(self.0.lock().unwrap().clone())
        }
    }

    struct MockTokens(Mutex<HashMap<String, StoredTokenData>>);

    impl MockTokens {
        fn new() -> std::sync::Arc<Self> {
            std::sync::Arc::new(Self(Mutex::new(HashMap::new())))
        }

        fn len(&self) -> usize {
            self.0.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl TokenStore for MockTokens {
        async fn store(
            &self,
            raw: &str,
            data: StoredTokenData,
            _ttl: u64,
        ) -> Result<(), DomainError> {
            self.0.lock().unwrap().insert(raw.to_string(), data);
            Ok(())
        }

        async fn get(&self, raw: &str) -> Result<Option<StoredTokenData>, DomainError> {
            Ok(self.0.lock().unwrap().get(raw).cloned())
        }

        async fn revoke(&self, raw: &str) -> Result<(), DomainError> {
            self.0.lock().unwrap().remove(raw);
            Ok(())
        }
    }

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn jwt_config() -> JwtConfig {
        JwtConfig {
            secret: "test-secret-minimum-32-chars-xxxxxxxxxxx".into(),
            access_token_expiry_secs: 900,
            refresh_token_expiry_secs: 604_800,
        }
    }

    type Fixture = (
        std::sync::Arc<MockUsers>,
        std::sync::Arc<MockTenants>,
        std::sync::Arc<MockTokens>,
        AuthService,
    );

    fn fixture() -> Fixture {
        let users = MockUsers::new();
        let tenants = MockTenants::new();
        let tokens = MockTokens::new();
        let svc = AuthService::new(
            std::sync::Arc::clone(&users) as _,
            std::sync::Arc::clone(&tenants) as _,
            std::sync::Arc::clone(&tokens) as _,
            jwt_config(),
        );
        (users, tenants, tokens, svc)
    }

    fn register_cmd(email: &str) -> RegisterCommand {
        RegisterCommand {
            business_name: "Test Bistro".into(),
            email: email.into(),
            password: "securepassword99".into(),
        }
    }

    // -----------------------------------------------------------------------
    // Register
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn register_creates_one_tenant_and_one_owner_user() {
        let (users, tenants, _, svc) = fixture();
        svc.register(register_cmd("a@test.com")).await.unwrap();

        assert_eq!(tenants.0.lock().unwrap().len(), 1);
        let u = users.0.lock().unwrap()[0].clone();
        assert_eq!(u.email, "a@test.com");
        assert_eq!(u.role, UserRole::Owner);
    }

    #[tokio::test]
    async fn register_stores_argon2_hash_not_plaintext() {
        let (users, _, _, svc) = fixture();
        svc.register(register_cmd("b@test.com")).await.unwrap();

        let hash = users.0.lock().unwrap()[0].password_hash.clone();
        assert_ne!(hash, "securepassword99", "must not store plaintext");
        assert!(hash.starts_with("$argon2"), "must be an Argon2 hash");
    }

    #[tokio::test]
    async fn register_returns_non_empty_token_pair() {
        let (_, _, _, svc) = fixture();
        let pair = svc.register(register_cmd("c@test.com")).await.unwrap();

        assert!(
            !pair.access_token.is_empty(),
            "access_token must not be empty"
        );
        assert!(
            !pair.refresh_token.is_empty(),
            "refresh_token must not be empty"
        );
        assert_eq!(pair.expires_in, 900);
    }

    #[tokio::test]
    async fn register_stores_refresh_token_in_store() {
        let (_, _, tokens, svc) = fixture();
        svc.register(register_cmd("d@test.com")).await.unwrap();
        assert_eq!(tokens.len(), 1, "one refresh token should be stored");
    }

    // -----------------------------------------------------------------------
    // Login
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn login_with_correct_password_returns_token_pair() {
        let (_, _, _, svc) = fixture();
        svc.register(register_cmd("e@test.com")).await.unwrap();

        let pair = svc
            .login(LoginCommand {
                email: "e@test.com".into(),
                password: "securepassword99".into(),
            })
            .await;

        assert!(
            pair.is_ok(),
            "login should succeed with correct credentials"
        );
    }

    #[tokio::test]
    async fn login_with_wrong_password_returns_invalid_credentials() {
        let (_, _, _, svc) = fixture();
        svc.register(register_cmd("f@test.com")).await.unwrap();

        let err = svc
            .login(LoginCommand {
                email: "f@test.com".into(),
                password: "wrongpassword".into(),
            })
            .await
            .unwrap_err();

        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[tokio::test]
    async fn login_with_unknown_email_returns_invalid_credentials() {
        let (_, _, _, svc) = fixture();

        let err = svc
            .login(LoginCommand {
                email: "nobody@test.com".into(),
                password: "anything".into(),
            })
            .await
            .unwrap_err();

        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    // -----------------------------------------------------------------------
    // Refresh
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn refresh_with_valid_token_returns_new_pair() {
        let (_, _, _, svc) = fixture();
        let first = svc.register(register_cmd("g@test.com")).await.unwrap();

        let second = svc
            .refresh(RefreshCommand {
                refresh_token: first.refresh_token.clone(),
            })
            .await
            .unwrap();

        // The refresh token must always be a new UUID — this is the rotation guarantee.
        assert_ne!(
            first.refresh_token, second.refresh_token,
            "refresh token must be rotated on every use",
        );
        // The access token carries a second-precision timestamp, so it may
        // be identical if both tokens are issued within the same wall-clock
        // second.  What matters is that the new pair was issued at all.
        assert!(!second.access_token.is_empty());
        assert_eq!(second.expires_in, 900);
    }

    #[tokio::test]
    async fn used_refresh_token_is_revoked_and_cannot_be_reused() {
        let (_, _, _, svc) = fixture();
        let first = svc.register(register_cmd("h@test.com")).await.unwrap();

        // First use succeeds.
        svc.refresh(RefreshCommand {
            refresh_token: first.refresh_token.clone(),
        })
        .await
        .unwrap();

        // Second use with the same (now-rotated) token must fail.
        let err = svc
            .refresh(RefreshCommand {
                refresh_token: first.refresh_token,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, AuthError::InvalidRefreshToken));
    }

    #[tokio::test]
    async fn refresh_with_garbage_token_returns_invalid_refresh_token() {
        let (_, _, _, svc) = fixture();

        let err = svc
            .refresh(RefreshCommand {
                refresh_token: "this-is-not-a-real-token".into(),
            })
            .await
            .unwrap_err();

        assert!(matches!(err, AuthError::InvalidRefreshToken));
    }

    // -----------------------------------------------------------------------
    // Logout
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn logout_removes_token_from_store() {
        let (_, _, tokens, svc) = fixture();
        let pair = svc.register(register_cmd("i@test.com")).await.unwrap();

        assert_eq!(tokens.len(), 1);
        svc.logout(LogoutCommand {
            refresh_token: pair.refresh_token,
        })
        .await
        .unwrap();
        assert_eq!(tokens.len(), 0, "token should be removed after logout");
    }

    #[tokio::test]
    async fn logout_then_refresh_returns_invalid_refresh_token() {
        let (_, _, _, svc) = fixture();
        let pair = svc.register(register_cmd("j@test.com")).await.unwrap();

        svc.logout(LogoutCommand {
            refresh_token: pair.refresh_token.clone(),
        })
        .await
        .unwrap();

        let err = svc
            .refresh(RefreshCommand {
                refresh_token: pair.refresh_token,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, AuthError::InvalidRefreshToken));
    }

    #[tokio::test]
    async fn double_logout_is_idempotent() {
        let (_, _, _, svc) = fixture();
        let pair = svc.register(register_cmd("k@test.com")).await.unwrap();

        svc.logout(LogoutCommand {
            refresh_token: pair.refresh_token.clone(),
        })
        .await
        .unwrap();
        // Second logout must not panic or error.
        svc.logout(LogoutCommand {
            refresh_token: pair.refresh_token,
        })
        .await
        .unwrap();
    }
}
