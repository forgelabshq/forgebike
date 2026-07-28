# Phase 1 — Auth & Multi-Tenancy

> **Status**: Complete  
> **Timeframe**: Week 2  
> **Exit criterion**: Register, login, refresh, logout, and `GET /me` all work correctly; wrong credentials return 401; revoked tokens can't be refreshed; clippy clean.

---

## What Was Built

| Deliverable | Location |
|---|---|
| `forgebike-application` crate | `crates/application/` |
| `forgebike-infrastructure` crate | `crates/infrastructure/` |
| Domain entities: `Tenant`, `User`, `AuthIdentity` | `crates/domain/src/entities/` |
| Port traits: `UserRepository`, `TenantRepository`, `TokenStore` | `crates/domain/src/ports/` |
| `AuthService` — all four auth use cases | `crates/application/src/auth/service.rs` |
| `PgUserRepository`, `PgTenantRepository` | `crates/infrastructure/src/db/` |
| `RedisTokenStore` | `crates/infrastructure/src/redis/` |
| Auth middleware `require_auth` | `crates/api/src/middleware/auth.rs` |
| Role extractors `RequireOwner`, `RequireManager` | `crates/api/src/extractors/role.rs` |
| `ValidatedJson<T>` extractor | `crates/api/src/extractors/validated_json.rs` |
| Auth handlers: register / login / refresh / logout / me | `crates/api/src/handlers/auth.rs` |
| Rate limiting (5 req burst, 1/s) on auth routes | `crates/api/src/router.rs` |
| `JwtConfig` added to config | `crates/config/src/lib.rs` |
| `[jwt]` section in `config/default.toml` | `config/default.toml` |
| `AppState` extended with `auth_service` | `crates/api/src/state.rs` |

---

## Architecture Decisions

### Ports & Adapters in Practice

Phase 1 is the first real test of the hexagonal architecture. The three
domain ports (`UserRepository`, `TenantRepository`, `TokenStore`) are defined
in `forgebike-domain` with `async_trait`; the concrete implementations live in
`forgebike-infrastructure`. The application service (`AuthService`) depends
only on the traits — it never imports `sqlx` or `deadpool_redis`.

This means:
1. Swapping PostgreSQL for SQLite (e.g., for testing) requires only a new
   struct that implements `UserRepository` — no changes to `AuthService`.
2. Swapping Redis for an in-memory store for integration tests is equally
   trivial.

### Token Strategy

| Token | Where stored | How stored | Lifetime |
|---|---|---|---|
| Access token (JWT) | Client-side only | Signed HS256, not persisted | 15 min |
| Refresh token | Redis | SHA-256 hash of the raw UUID, TTL enforced | 7 days |

**Why hash the refresh token in Redis?**
The raw token is equivalent to a credential — if the Redis store is
compromised, a stolen raw token could be replayed. Storing only the SHA-256
hash means a leaked store is useless without the original tokens. The
hashing happens entirely inside `RedisTokenStore`; the application layer
never sees a hash.

**Why rotate refresh tokens?**
On every call to `POST /refresh`, the old refresh token is revoked and a new
one is issued. This limits the damage if an old token is intercepted after use.

### Password Hashing

Argon2id via the `argon2` crate (`argon2 0.5`). Argon2id is the current
recommended algorithm — it is memory-hard (resists GPU/ASIC attacks) and uses
a random salt per hash so identical passwords produce different hashes.
Parameters: the crate defaults (`m=19456 KiB, t=2, p=1`).

### JWT

HS256 (HMAC-SHA256) using the `jsonwebtoken 9` crate. The secret comes from
`APP__JWT__SECRET` (never committed). The server warns loudly at startup if
the development placeholder secret is in use.

**Claims:**

```
{
  "sub": "<user_id UUID>",
  "tenant_id": "<tenant_id UUID>",
  "role": "owner | manager | viewer",
  "iat": <unix timestamp>,
  "exp": <unix timestamp>
}
```

### Multi-Tenancy

Every table has had `tenant_id` since Phase 0. In Phase 1:

- `POST /register` always creates a **new** tenant. The same email can
  register multiple times if the owner runs more than one restaurant group.
- `POST /login` looks up the user globally by email. If a user email exists
  in multiple tenants (possible but uncommon), the first record found is
  returned. A tenant-selector flow can be added in a future phase.
- The JWT carries `tenant_id`, so every downstream request is scoped to the
  correct tenant without any extra DB lookup.
- Repository methods accept `TenantId` as a required parameter — the compiler
  prevents accidental cross-tenant queries.

### Auth Middleware

`require_auth` is a Tower middleware function applied with
`middleware::from_fn_with_state`. It:

1. Extracts the `Authorization: Bearer <token>` header.
2. Decodes and validates the JWT (signature + expiry).
3. Parses `sub`, `tenant_id`, and `role` back into typed domain values.
4. Inserts an `AuthIdentity` into the request extensions.

Protected handlers extract this with axum's built-in
`Extension<AuthIdentity>` extractor, which returns `401` if the identity is
absent (i.e., the middleware was not applied or the token was invalid).

### Rate Limiting

`tower_governor 0.4` is applied only to the four public auth routes
(`/register`, `/login`, `/refresh`, `/logout`). The configuration allows a
burst of 5 requests, refilling at 1 per second per client IP.

`axum::serve` must be called with
`.into_make_service_with_connect_info::<SocketAddr>()` so that
tower_governor can read the client's socket address.

---

## Updated Crate Dependency Graph

```
forgebike-server  (binary — composition root)
    ├── forgebike-api
    │       ├── forgebike-config
    │       ├── forgebike-domain
    │       └── forgebike-application
    │               ├── forgebike-config
    │               └── forgebike-domain
    ├── forgebike-infrastructure
    │       └── forgebike-domain
    └── forgebike-config

forgebike-domain  ← zero internal deps
```

---

## API Reference

### `POST /api/v1/auth/register`

Creates a new tenant and its first owner user.

**Request body:**
```json
{
  "business_name": "Bistro 42",
  "email": "chef@bistro42.com",
  "password": "hunter2secret"
}
```

**Validation:** `business_name` 1–200 chars; `email` valid format; `password`
≥ 8 chars.

**Response `201 Created`:**
```json
{
  "access_token": "eyJ...",
  "refresh_token": "deeb1a8a-...",
  "expires_in": 900,
  "token_type": "Bearer"
}
```

---

### `POST /api/v1/auth/login`

**Request body:**
```json
{ "email": "chef@bistro42.com", "password": "hunter2secret" }
```

**Response `200 OK`:** same shape as register.  
**Response `401`:** `{"error": "Invalid email or password"}` — identical
message whether the email doesn't exist or the password is wrong (avoids
user enumeration).

---

### `POST /api/v1/auth/refresh`

Rotates the refresh token. The old token is immediately revoked.

**Request body:**
```json
{ "refresh_token": "deeb1a8a-..." }
```

**Response `200 OK`:** new token pair.  
**Response `401`:** token not found or expired.

---

### `POST /api/v1/auth/logout`

Revokes the refresh token. Returns `204 No Content` even if the token is
already expired — no information leakage about session state.

**Request body:**
```json
{ "refresh_token": "deeb1a8a-..." }
```

---

### `GET /api/v1/auth/me`

Returns the authenticated user's identity. Requires `Authorization: Bearer
<access_token>` header.

**Response `200 OK`:**
```json
{
  "user_id": "ea3dd882-...",
  "tenant_id": "fb787367-...",
  "role": "owner"
}
```

**Response `401`:** missing or invalid token.

---

## Environment Variables Added

| Variable | Default | Description |
|---|---|---|
| `APP__JWT__SECRET` | *(dev placeholder — CHANGE IT)* | HS256 signing secret |
| `APP__JWT__ACCESS_TOKEN_EXPIRY_SECS` | `900` | Access token lifetime (seconds) |
| `APP__JWT__REFRESH_TOKEN_EXPIRY_SECS` | `604800` | Refresh token lifetime (seconds) |

Generate a production secret with:

```sh
openssl rand -hex 32
```

Then set it as `APP__JWT__SECRET` in your deployment environment. Never
commit it.

---

## New Dependencies

| Crate | Version | Used in | Purpose |
|---|---|---|---|
| `async-trait` | 0.1 | domain, infra | `async fn` in `dyn`-compatible traits |
| `argon2` | 0.5 | application | Argon2id password hashing |
| `jsonwebtoken` | 9 | application, api | JWT encode/decode |
| `tower_governor` | 0.4 | api | Per-IP rate limiting on auth routes |
| `sha2` | 0.10 | infrastructure | SHA-256 hash of refresh tokens before Redis storage |
| `hex` | 0.4 | infrastructure | Hex-encode the SHA-256 hash |

---

## What Phase 2 Will Add

- `crates/application/src/restaurant/` — `RestaurantService`
- Full CRUD for `Restaurant` (name, cuisine type, address, external IDs)
- Full CRUD for `MenuItem` (with cursor-based pagination)
- All endpoints guarded with `require_auth` and scoped by `TenantId`
- `GET /api/v1/restaurants` — list restaurants for the authenticated tenant
- Migrations: `restaurants`, `menu_items` tables
