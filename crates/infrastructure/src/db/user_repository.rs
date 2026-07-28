//! `sqlx`-backed implementation of [`UserRepository`].
//!
//! # Type mapping
//! `PostgreSQL` `user_role` and `plan_tier` are native ENUM types.  To avoid
//! adding `sqlx::Type` to the domain enums, all queries cast them to `TEXT`
//! on the way out and to `::user_role` on the way in.  This keeps the domain
//! completely free of database concerns.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use forgebike_domain::{
    entities::user::{User, UserRole},
    identifiers::{TenantId, UserId},
    ports::user_repository::{NewUser, UserRepository},
    DomainError,
};

// ---------------------------------------------------------------------------
// DB row — internal, never exposed outside this module
// ---------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    tenant_id: Uuid,
    email: String,
    password_hash: String,
    role: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<UserRow> for User {
    type Error = DomainError;

    fn try_from(row: UserRow) -> Result<Self, Self::Error> {
        let role = row
            .role
            .parse::<UserRole>()
            .map_err(DomainError::Internal)?;

        Ok(User {
            id: UserId::from_uuid(row.id),
            tenant_id: TenantId::from_uuid(row.tenant_id),
            email: row.email,
            password_hash: row.password_hash,
            role,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

pub struct PgUserRepository {
    pool: PgPool,
}

impl PgUserRepository {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn create(&self, new_user: NewUser) -> Result<User, DomainError> {
        let row = sqlx::query_as::<_, UserRow>(
            r"
            INSERT INTO users (tenant_id, email, password_hash, role)
            VALUES ($1, $2, $3, $4::user_role)
            RETURNING
                id,
                tenant_id,
                email,
                password_hash,
                role::TEXT AS role,
                created_at,
                updated_at
            ",
        )
        .bind(new_user.tenant_id.as_uuid())
        .bind(&new_user.email)
        .bind(&new_user.password_hash)
        .bind(new_user.role.to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            // Postgres unique-violation code is 23505
            if let sqlx::Error::Database(ref db_err) = e {
                if db_err.code().as_deref() == Some("23505") {
                    return DomainError::Conflict(
                        "Email already registered for this tenant".into(),
                    );
                }
            }
            DomainError::Internal(e.to_string())
        })?;

        User::try_from(row)
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<User>, DomainError> {
        let row = sqlx::query_as::<_, UserRow>(
            r"
            SELECT
                id,
                tenant_id,
                email,
                password_hash,
                role::TEXT AS role,
                created_at,
                updated_at
            FROM users
            WHERE email = $1
            LIMIT 1
            ",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        row.map(User::try_from).transpose()
    }

    async fn find_by_id(&self, id: UserId) -> Result<Option<User>, DomainError> {
        let row = sqlx::query_as::<_, UserRow>(
            r"
            SELECT
                id,
                tenant_id,
                email,
                password_hash,
                role::TEXT AS role,
                created_at,
                updated_at
            FROM users
            WHERE id = $1
            ",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DomainError::Internal(e.to_string()))?;

        row.map(User::try_from).transpose()
    }
}
