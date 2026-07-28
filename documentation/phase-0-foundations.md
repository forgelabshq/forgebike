# Phase 0 — Project Foundations

> **Status**: Complete  
> **Timeframe**: Week 1  
> **Exit criterion**: `cargo build` succeeds; `GET /health` returns `200 OK`; CI pipeline is green.

---

## What Was Built

Phase 0 is the skeleton that every subsequent phase builds on. Nothing in the
product is user-visible at this stage — the goal is correct wiring, not
features.

| Deliverable | Location |
|---|---|
| Cargo workspace manifest | `Cargo.toml` |
| `forgebike-config` crate | `crates/config/` |
| `forgebike-domain` crate | `crates/domain/` |
| `forgebike-api` crate | `crates/api/` |
| `forgebike-server` binary | `crates/server/` |
| PostgreSQL migrations (0001–0002) | `migrations/` |
| Default configuration file | `config/default.toml` |
| Docker Compose stack | `docker-compose.yml` |
| Environment variable template | `.env.example` |
| GitHub Actions CI pipeline | `.github/workflows/ci.yml` |

---

## Crate Dependency Graph

```
forgebike-server  (binary — wires everything together)
    ├── forgebike-api
    │       ├── forgebike-config
    │       └── forgebike-domain
    └── forgebike-config

forgebike-domain  (no internal deps — pure Rust + serde + uuid)
```

Dependencies always flow **inward**. `forgebike-domain` knows nothing about
HTTP, databases, or configuration. `forgebike-api` knows about HTTP but not
about the database connection pool directly (it receives it via `AppState`).
`forgebike-server` is the only crate allowed to know about every layer — it
is the composition root.

---

## Architecture Decisions Made in This Phase

### Cargo Workspace

The project uses a multi-crate Cargo workspace rather than a single flat
crate. This enforces architectural boundaries at the **compiler** level — you
cannot accidentally import a database function into the domain crate because
there is no declared dependency between them.

Compile times also benefit: unchanged crates do not need to be recompiled,
which matters as the codebase grows.

### `axum` + `tokio`

`axum` was chosen as the web framework because:
- It is built directly on `tokio` and `tower`, which are the de-facto async
  standards in the Rust ecosystem.
- Middleware is composed via `tower::Layer`, which is reusable and testable
  independently of the web framework.
- Its `State` extractor is ergonomic and type-safe.
- It has no global state or thread-local magic, making testing straightforward.

### `sqlx` for Database Access

`sqlx` was chosen over `diesel` and `sea-orm` because:
- It is fully async and works naturally with `tokio`.
- SQL is written as plain SQL strings — no DSL to learn, queries are
  transparent and easy to review.
- `sqlx::query!()` macros (used from Phase 2 onwards) verify SQL at compile
  time against a live database (or a cached `.sqlx/` snapshot for CI).

### Hexagonal Architecture via Rust Traits

Every external dependency (database, AI, review platforms) is hidden behind a
trait defined in `forgebike-domain`. Concrete implementations live in future
`crates/infrastructure/*` crates. This means:

1. The domain layer has zero third-party dependencies beyond utilities
   (`serde`, `uuid`, `thiserror`).
2. Any infrastructure component can be replaced with a mock in tests by
   implementing the trait.
3. The compiler enforces the dependency direction — infrastructure depends on
   domain, never the other way around.

### Newtype ID Wrappers

Every aggregate root has its own ID type (`TenantId`, `RestaurantId`, etc.)
that wraps `uuid::Uuid`. This prevents the common bug of passing a `UserId`
where a `TenantId` is expected — the compiler rejects it at zero runtime cost.

### Multi-tenant Security by Design

The `tenant_id` column is present on every table from the very first
migration. Repository methods (added in Phase 2+) will always accept a
`TenantId` as a required argument, and that `TenantId` is extracted from the
authenticated JWT by the auth middleware — never trusted from user input.

---

## Local Development Setup

### Prerequisites

- Rust stable (≥ 1.75) — install via [rustup](https://rustup.rs)
- Docker + Docker Compose
- `sqlx-cli` (optional but useful for manual migration management)

```sh
# Install sqlx-cli
cargo install sqlx-cli --no-default-features --features rustls,postgres

# Useful dev tools
cargo install cargo-watch   # hot-reload
cargo install cargo-nextest  # faster test runner
```

### First-time setup

```sh
# 1. Clone and enter the project
cd forgebike

# 2. Copy the environment template
cp .env.example .env
# Edit .env if your local ports differ from the defaults

# 3. Start Postgres + Redis
docker compose up -d

# 4. Build and run the server
cargo run --bin forgebike
```

The server starts on `http://0.0.0.0:8080`.

### Verify it works

```sh
curl http://localhost:8080/health
```

Expected response when both services are up:

```json
{
  "status": "ok",
  "components": {
    "database": "ok",
    "redis": "ok"
  }
}
```

If either service is down you will receive `503 Service Unavailable` with
`"status": "degraded"` and the failing component marked `"error"`.

### Hot-reload during development

```sh
cargo watch -x 'run --bin forgebike'
```

---

## Configuration

Configuration is loaded in layers (each layer overrides the previous):

| Layer | Source | Committed? |
|---|---|---|
| 1 | `config/default.toml` | Yes — no secrets |
| 2 | `config/{APP_ENV}.toml` | No — local overrides |
| 3 | Environment variables (`APP__*`) | Via platform / `.env` |

### Variable reference

| Variable | Default | Description |
|---|---|---|
| `APP_ENV` | `development` | Selects per-env config file and log format |
| `APP__SERVER__HOST` | `0.0.0.0` | Bind address |
| `APP__SERVER__PORT` | `8080` | Bind port |
| `DATABASE_URL` | *(see default.toml)* | Full Postgres connection string |
| `APP__DATABASE__MAX_CONNECTIONS` | `10` | Pool size |
| `APP__REDIS__URL` | `redis://127.0.0.1:6379` | Redis connection string |
| `APP__APP__LOG_LEVEL` | `info` | `trace`, `debug`, `info`, `warn`, `error` |
| `RUST_LOG` | — | Alternative log filter (used if `APP__APP__LOG_LEVEL` unset) |

---

## Database Migrations

Migrations are located in `migrations/` and run automatically at server
startup via `sqlx::migrate!()`. They can also be run manually:

```sh
# Apply all pending migrations
sqlx migrate run --database-url postgres://postgres:password@localhost:5432/forgebike

# Check migration status
sqlx migrate info --database-url postgres://postgres:password@localhost:5432/forgebike

# Revert the last migration
sqlx migrate revert --database-url postgres://postgres:password@localhost:5432/forgebike
```

### Migrations in this phase

| File | Creates |
|---|---|
| `20250101000001_create_tenants.sql` | `tenants` table, `plan_tier` enum, `set_updated_at()` trigger function |
| `20250101000002_create_users.sql` | `users` table, `user_role` enum, `refresh_tokens` table |

---

## CI Pipeline

Three jobs run in parallel on every push and PR:

```
push / PR
    ├── check   → cargo fmt, cargo clippy, cargo check  (no DB needed)
    ├── test    → cargo nextest (with real Postgres + Redis services)
    └── audit   → cargo audit (CVE scan of dependency tree)
```

The `check` job sets `SQLX_OFFLINE=true` so that `sqlx` compile-time query
verification does not require a live database connection during linting. The
`test` job unsets this and runs against a real database.

---

## File Structure After Phase 0

```
forgebike/
├── Cargo.toml                          ← workspace manifest
├── docker-compose.yml
├── .env.example
├── .github/
│   └── workflows/
│       └── ci.yml
├── config/
│   └── default.toml
├── crates/
│   ├── config/                         ← forgebike-config
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── domain/                         ← forgebike-domain
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── error.rs
│   │       └── identifiers.rs
│   ├── api/                            ← forgebike-api
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── state.rs
│   │       ├── error.rs
│   │       ├── router.rs
│   │       └── handlers/
│   │           ├── mod.rs
│   │           └── health.rs
│   └── server/                         ← forgebike-server (binary)
│       ├── Cargo.toml
│       └── src/main.rs
├── documentation/
│   ├── architecture.md
│   └── phase-0-foundations.md          ← this file
└── migrations/
    ├── 20250101000001_create_tenants.sql
    └── 20250101000002_create_users.sql
```

---

## What Phase 1 Will Add

Phase 1 introduces authentication and multi-tenancy on top of this skeleton:

- `crates/application/` — the first application service (`AuthService`)
- `POST /api/v1/auth/register` — creates a tenant + owner user
- `POST /api/v1/auth/login` — issues a JWT access token + Redis-backed refresh token
- `POST /api/v1/auth/refresh` — rotates the token pair
- `POST /api/v1/auth/logout` — revokes the refresh token
- `AuthLayer` axum middleware — validates the JWT and injects `TenantId` + `UserId`
  into every protected request's extensions
- Role-based guard extractors

See [`architecture.md`](./architecture.md) for the full multi-phase plan.
