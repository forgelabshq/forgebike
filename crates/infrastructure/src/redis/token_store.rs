//! Redis-backed implementation of [`TokenStore`].
//!
//! # Key format
//! `rt:{sha256_hex(raw_token)}`
//!
//! # Hashing strategy
//! Raw refresh tokens (UUID v4 strings) are SHA-256 hashed before being
//! written to or read from Redis.  Only the hash ever touches the store —
//! the raw token is kept client-side only, analogous to how passwords are
//! never stored in plaintext.
//!
//! # Value format
//! Values are stored as JSON: `{"user_id":"…","tenant_id":"…","role":"…"}`

use async_trait::async_trait;
use deadpool_redis::redis::AsyncCommands;
use deadpool_redis::Pool as RedisPool;
use hex::encode as hex_encode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use forgebike_domain::{
    entities::user::UserRole,
    identifiers::{TenantId, UserId},
    ports::token_store::{StoredTokenData, TokenStore},
    DomainError,
};

// ---------------------------------------------------------------------------
// Serialisable form stored in Redis
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct TokenValue {
    user_id: String,
    tenant_id: String,
    role: String,
}

impl TryFrom<TokenValue> for StoredTokenData {
    type Error = DomainError;

    fn try_from(v: TokenValue) -> Result<Self, Self::Error> {
        Ok(StoredTokenData {
            user_id: v
                .user_id
                .parse::<UserId>()
                .map_err(|_| DomainError::Internal("malformed user_id in token store".into()))?,
            tenant_id: v
                .tenant_id
                .parse::<TenantId>()
                .map_err(|_| DomainError::Internal("malformed tenant_id in token store".into()))?,
            role: v
                .role
                .parse::<UserRole>()
                .map_err(DomainError::Internal)?,
        })
    }
}

// ---------------------------------------------------------------------------
// Adapter
// ---------------------------------------------------------------------------

pub struct RedisTokenStore {
    pool: RedisPool,
}

impl RedisTokenStore {
    #[must_use] 
    pub fn new(pool: RedisPool) -> Self {
        Self { pool }
    }

    fn key(raw_token: &str) -> String {
        format!("rt:{}", hex_encode(Sha256::digest(raw_token.as_bytes())))
    }
}

#[async_trait]
impl TokenStore for RedisTokenStore {
    async fn store(
        &self,
        raw_token: &str,
        data: StoredTokenData,
        ttl_secs: u64,
    ) -> Result<(), DomainError> {
        let value = serde_json::to_string(&TokenValue {
            user_id: data.user_id.to_string(),
            tenant_id: data.tenant_id.to_string(),
            role: data.role.to_string(),
        })
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        conn.set_ex::<_, _, ()>(Self::key(raw_token), value, ttl_secs)
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))
    }

    async fn get(&self, raw_token: &str) -> Result<Option<StoredTokenData>, DomainError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        let raw: Option<String> = conn
            .get(Self::key(raw_token))
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        raw.map(|s| {
            let tv: TokenValue =
                serde_json::from_str(&s).map_err(|e| DomainError::Internal(e.to_string()))?;
            StoredTokenData::try_from(tv)
        })
        .transpose()
    }

    async fn revoke(&self, raw_token: &str) -> Result<(), DomainError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))?;

        conn.del::<_, ()>(Self::key(raw_token))
            .await
            .map_err(|e| DomainError::Internal(e.to_string()))
    }
}
