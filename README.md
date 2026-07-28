# Forgebike — Restaurant AI Platform

> AI-powered digital growth for restaurants. Automated review management,
> marketing content generation, customer engagement, and business intelligence
> — all delivered through a single multi-tenant API.

[![CI](https://github.com/forgelabshq/forgebike/actions/workflows/ci.yml/badge.svg)](https://github.com/forgelabshq/forgebike/actions/workflows/ci.yml)
![Rust](https://img.shields.io/badge/rust-stable-orange)
![License](https://img.shields.io/badge/license-UNLICENSED-red)

---

## What It Does

Forgebike is the backend for a SaaS platform that helps restaurant businesses
grow online. Each restaurant is an isolated tenant with its own users, data,
and subscription plan. The platform provides:

| Feature | Description |
|---|---|
| **Review Management** | Aggregate reviews from Google, Yelp, and TripAdvisor. AI-generated reply drafts. Sentiment scoring. |
| **AI Content Generation** | Social media posts, email copy, blog intros, and menu descriptions — generated and stored as drafts for human approval. |
| **Customer Engagement** | AI chat widget for restaurant websites. Email and SMS campaign scheduling. |
| **Business Intelligence** | KPI dashboards — rating trends, sentiment over time, content velocity, competitor snapshots. |
| **Billing** | Stripe-backed subscription tiers (`Starter`, `Growth`, `Scale`) with per-tenant AI usage metering. |

The Python frontend consumes everything through a versioned REST + JSON API,
with a WebSocket endpoint for the live chat widget.

---

## Tech Stack

### Core

| Concern | Choice | Why |
|---|---|---|
| Language | Rust (stable) | Performance, memory safety, excellent async story |
| Web framework | `axum` 0.7 | Built on Tokio + Tower; composable middleware; type-safe extractors |
| Async runtime | `tokio` | De-facto standard; required by axum |
| Database | PostgreSQL 16 | Row-level security, JSONB, full-text search |
| DB driver | `sqlx` 0.9 | Fully async; runtime-checked SQL (no compile-time DB connection required) |
| Cache / queues | Redis 7 via `deadpool-redis` | Refresh token store, rate-limit counters, future job queues |
| Migrations | `sqlx migrate` | Embedded, versioned SQL — run automatically at startup |
| Serialisation | `serde` + `serde_json` | Universal Rust standard |
| Config | `config` + `dotenvy` | Layered: file → env-file → `APP__*` env var overrides |
| Tracing | `tracing` + `tracing-subscriber` | Structured spans; pretty output in dev, JSON in production |
| Error handling | `thiserror` + `anyhow` | Typed domain errors; ergonomic propagation |

### Auth & Security

| Concern | Crate | Detail |
|---|---|---|
| JSON Web Tokens | `jsonwebtoken` 9 | HS256; access token 15 min, refresh token 7 days |
| Password hashing | `argon2` 0.5 | Argon2id with random salt — resistant to GPU/ASIC attacks |
| Rate limiting | `tower_governor` 0.4 | Per-IP token-bucket limiter on auth routes via Tower middleware |
| Input validation | `validator` 0.21 | Derive-macro validation; `ValidatedJson<T>` extractor returns `422` on failure |
| CORS | `tower-http` | `CorsLayer::permissive()` in development — tighten per-origin before production |

### External APIs

| Concern | Crate | Detail |
|---|---|---|
| HTTP client | `reqwest` 0.12 | Used by review platform clients; rustls-TLS, no OpenSSL dependency |
| Cursor encoding | `base64` 0.22 | URL-safe base64 (no padding) for opaque pagination cursors |

---

## Project Structure

```
forgebike/
├── Cargo.toml                   ← workspace manifest (all dep versions live here)
├── docker-compose.yml           ← local Postgres 16 + Redis 7
├── config/
│   └── default.toml             ← committed baseline config (no secrets)
├── migrations/                  ← versioned SQL, auto-applied at startup
├── scripts/
│   └── test.sh                  ← full-stack dev launcher + test runner
├── documentation/
│   ├── architecture.md
│   ├── phase-0-foundations.md
│   ├── phase-1-auth.md
│   ├── phase-2-restaurants.md
│   └── phase-3-reviews.md
└── crates/
    ├── config/                  ← layered config loading + typed structs
    ├── domain/                  ← entities, ID types, port traits, pagination
    ├── application/             ← use-case services (AuthService, RestaurantService, ReviewService)
    ├── infrastructure/          ← Postgres repos, Redis token store, external API clients
    ├── api/                     ← axum router, handlers, middleware, extractors
    └── server/                  ← binary entry point — wires everything together
```

### Crate dependency graph

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

forgebike-domain  ← zero internal deps (pure Rust + serde + uuid + chrono)
```

Dependencies always point **inward**. The domain layer has no knowledge of HTTP,
databases, or any external service. Architecture boundaries are enforced at the
compiler level — the `Cargo.toml` dependency graph makes it impossible to import
a database function into the domain crate.

---

## Quick Start

### Prerequisites

| Tool | Version | Install |
|---|---|---|
| Rust | stable | [rustup.rs](https://rustup.rs) |
| Docker | any recent | [docker.com](https://docker.com) |
| `sqlx-cli` | latest | `cargo install sqlx-cli --no-default-features --features rustls,postgres` |

Optional but recommended:

```sh
cargo install cargo-watch    # hot-reload on file changes
cargo install cargo-nextest  # faster parallel test runner
```

### 1. Configure environment

```sh
cp .env.example .env
```

The defaults work out of the box with the Docker Compose stack. If you already
have a native PostgreSQL on port 5432, use `127.0.0.1` (not `localhost`) in the
connection string to force TCP authentication:

```sh
# Native Postgres on 5432
DATABASE_URL=postgres://postgres:password@127.0.0.1:5432/forgebike

# Docker Compose Postgres (mapped to 5435 to avoid port conflicts)
DATABASE_URL=postgres://postgres:password@127.0.0.1:5435/forgebike
```

### 2. Start infrastructure

```sh
docker compose up -d
```

Starts:
- **PostgreSQL 16** on host port `5435` (dev) and `5436` (test)
- **Redis 7** on host port `6379`

> Docker Compose uses ports `5435`/`5436` deliberately to avoid conflicting
> with a native Postgres instance that may already be on `5432`.

### 3. Run the server

```sh
cargo run --bin forgebike
```

Database migrations run automatically at startup. The server listens on
`http://0.0.0.0:8080` and shuts down gracefully on `Ctrl-C` or `SIGTERM`.

### 4. Verify

```sh
curl http://localhost:8080/health
```

```json
{
  "status": "ok",
  "components": {
    "database": "ok",
    "redis": "ok"
  }
}
```

### Hot reload

```sh
cargo watch -x 'run --bin forgebike'
```

### Full-stack test runner

The `scripts/test.sh` script starts infrastructure, starts the server, waits
for it to be ready, runs the full curl-based test suite, and then leaves the
server running for development:

```sh
./scripts/test.sh                  # start everything, run tests, keep running
TEARDOWN=true ./scripts/test.sh    # same but stop everything when done
BASE_URL=http://... ./scripts/test.sh  # run against a different host
```

---

## Configuration

Config is loaded in layers — each layer overrides the one before it:

| Priority | Source | Committed? |
|---|---|---|
| 1 (lowest) | `config/default.toml` | Yes — no secrets |
| 2 | `config/{APP_ENV}.toml` | No — local overrides, gitignored |
| 3 (highest) | `APP__*` environment variables | Via platform or `.env` |

`DATABASE_URL` (bare, without the `APP__` prefix) is also accepted and
automatically re-mapped — keeping compatibility with Railway, Fly.io, Render,
Heroku, and the `sqlx` CLI.

### All configuration variables

| Variable | Default | Description |
|---|---|---|
| `APP_ENV` | `development` | Selects log format (pretty/JSON) and per-env config file |
| `DATABASE_URL` | see `default.toml` | Postgres connection string |
| `APP__DATABASE__MAX_CONNECTIONS` | `10` | `PgPool` size |
| `APP__REDIS__URL` | `redis://127.0.0.1:6379` | Redis connection string |
| `APP__SERVER__HOST` | `0.0.0.0` | Bind address |
| `APP__SERVER__PORT` | `8080` | Bind port |
| `APP__APP__LOG_LEVEL` | `info` | `trace` · `debug` · `info` · `warn` · `error` |
| `RUST_LOG` | — | Fallback log filter (used if `APP__APP__LOG_LEVEL` is not set) |
| `APP__JWT__SECRET` | *(dev placeholder)* | HS256 signing secret — **must be changed in production** (`openssl rand -hex 32`) |
| `APP__JWT__ACCESS_TOKEN_EXPIRY_SECS` | `900` | Access token lifetime (15 minutes) |
| `APP__JWT__REFRESH_TOKEN_EXPIRY_SECS` | `604800` | Refresh token lifetime (7 days) |
| `APP__RATE_LIMIT__BURST_SIZE` | `5` | Initial token-bucket capacity for the auth rate limiter |
| `APP__RATE_LIMIT__PER_SECOND` | `1` | Token refill rate (requests per second per IP) |
| `APP__EXTERNAL_APIS__GOOGLE_PLACES_API_KEY` | `""` | Google Cloud Console → Places API |
| `APP__EXTERNAL_APIS__YELP_API_KEY` | `""` | Yelp Fusion portal → Manage App |
| `APP__EXTERNAL_APIS__TRIPADVISOR_API_KEY` | `""` | TripAdvisor Content API (partnership required) |

External API keys default to empty strings. When empty, the corresponding
review platform is silently skipped during sync — no error is raised.

---

## Security Features

### Authentication

- **JWT (HS256)** — short-lived access tokens (15 min) + Redis-backed refresh
  tokens (7 days). Refresh tokens are SHA-256 hashed before storage so a
  leaked Redis dump cannot be replayed.
- **Token rotation** — every `POST /auth/refresh` call issues a new refresh
  token and immediately revokes the old one.
- **Argon2id** — industry-recommended memory-hard password hashing; resists
  GPU and ASIC brute-force attacks.
- **No user enumeration** — wrong password and unknown email return identical
  `401` responses with identical error messages.

### Rate Limiting

Auth endpoints (`/register`, `/login`, `/refresh`, `/logout`) are protected by
`tower_governor` with a per-IP token-bucket limiter (5-request burst, 1/s
steady-state by default). Configurable via `APP__RATE_LIMIT__*` for CI and
load-testing environments.

### Role-Based Access Control

The `AuthIdentity` (user ID, tenant ID, role) decoded from the JWT is injected
into axum request extensions by the `require_auth` middleware. Handlers can
then use typed role guard extractors:

| Extractor | Admitted roles | Rejection |
|---|---|---|
| `Extension<AuthIdentity>` | Any authenticated user | `401` if not authenticated |
| `RequireManager` | `manager` or `owner` | `403` if `viewer` |
| `RequireOwner` | `owner` only | `403` if `manager` or `viewer` |

### Input Validation

The `ValidatedJson<T>` extractor deserialises the JSON body and then runs
`validator::Validate` on it. Any constraint violation (email format, minimum
length, range, etc.) returns `422 Unprocessable Entity` before the handler
executes.

### `unsafe_code = "forbid"`

The entire workspace prohibits `unsafe` code at the compiler level.

---

## API Endpoints

### Currently implemented (Phases 0–3)

| Method | Path | Auth | Description |
|---|---|---|---|
| `GET` | `/health` | None | Liveness probe — checks DB and Redis |
| `POST` | `/api/v1/auth/register` | None | Create tenant + owner user, return token pair |
| `POST` | `/api/v1/auth/login` | None | Verify credentials, return token pair |
| `POST` | `/api/v1/auth/refresh` | None | Rotate refresh token, return new pair |
| `POST` | `/api/v1/auth/logout` | None | Revoke refresh token |
| `GET` | `/api/v1/auth/me` | Bearer | Return authenticated user identity |
| `POST` | `/api/v1/restaurants` | Bearer | Create a restaurant |
| `GET` | `/api/v1/restaurants` | Bearer | List restaurants (cursor-paginated) |
| `GET` | `/api/v1/restaurants/:id` | Bearer | Get a restaurant |
| `PATCH` | `/api/v1/restaurants/:id` | Bearer | Partial update a restaurant |
| `DELETE` | `/api/v1/restaurants/:id` | Bearer | Delete restaurant + all its menu items |
| `POST` | `/api/v1/restaurants/:id/menu` | Bearer | Add a menu item |
| `GET` | `/api/v1/restaurants/:id/menu` | Bearer | List menu items (cursor-paginated) |
| `PATCH` | `/api/v1/restaurants/:id/menu/:item_id` | Bearer | Partial update a menu item |
| `DELETE` | `/api/v1/restaurants/:id/menu/:item_id` | Bearer | Delete a menu item |
| `POST` | `/api/v1/restaurants/:id/reviews/sync` | Bearer | Sync reviews from Google / Yelp |
| `GET` | `/api/v1/restaurants/:id/reviews` | Bearer | List reviews (newest-first, filterable) |

### Cursor-based pagination

All list endpoints (`/restaurants`, `/restaurants/:id/menu`,
`/restaurants/:id/reviews`) use cursor-based pagination instead of page
offsets. This prevents the "skipped row" and "duplicate row" bugs that occur
when rows are inserted between page fetches.

**Request:** `GET /api/v1/restaurants?limit=20&cursor=<opaque_string>`

**Response:**
```json
{
  "items": [ ... ],
  "next_cursor": "MTczNjM4MDgwMDAwMDo..."
}
```

Pass the value of `next_cursor` back as `cursor` to fetch the next page.
`next_cursor` is `null` when there are no further pages. An invalid or
tampered cursor silently resets to the first page.

### Review filters

The review list endpoint supports additional query parameters:

| Param | Type | Description |
|---|---|---|
| `platform` | string | `google` · `yelp` · `tripadvisor` |
| `min_rating` | integer | Minimum star rating (1–5) |
| `from` | RFC 3339 datetime | Earliest published date |
| `to` | RFC 3339 datetime | Latest published date |

### Planned (see `documentation/architecture.md` for the full surface)

```
POST   /api/v1/restaurants/:id/reviews/:rid/reply-draft

POST   /api/v1/restaurants/:id/content/generate
GET    /api/v1/restaurants/:id/content

GET    /api/v1/restaurants/:id/analytics/overview

WS     /api/v1/ws/chat/:restaurant_id
```

---

## Development

### Running tests

```sh
./scripts/test.sh          # full stack (starts Docker + server, runs all tests)

cargo test --all-targets   # unit tests only (no server needed)
cargo nextest run          # same but faster with nextest
```

The unit test suite (55 tests) runs entirely in-memory with mock
implementations of the port traits — no database or network required.

The `scripts/test.sh` integration suite (101 assertions) starts the server,
runs curl-based tests against the live API, and leaves the server running for
further development.

### Linting and formatting

```sh
cargo fmt --all                             # format in place
cargo fmt --all --check                     # check only (used in CI)
cargo clippy --all-targets -- -D warnings   # lint with warnings as errors
```

The workspace enforces `clippy::pedantic` at warning level and
`unsafe_code = "forbid"` at error level.

### Adding a migration

```sh
sqlx migrate add <migration_name>
# Edit the generated file in migrations/
# Migrations run automatically on next `cargo run`
```

### Checking migration status

```sh
sqlx migrate info --database-url postgres://postgres:password@127.0.0.1:5432/forgebike
```

---

## CI Pipeline

Four jobs run on **every push to every branch** and on pull requests targeting
`main` or `develop`. In-progress runs for the same branch are automatically
cancelled when a newer commit is pushed.

| Job | What it checks | Services needed |
|---|---|---|
| **Check & Lint** | `cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo check` | None (`SQLX_OFFLINE=true`) |
| **Release Build** | `cargo build --release --bin forgebike` | None |
| **Tests** | `cargo nextest run` against real Postgres 16 + Redis 7 | GitHub Actions services |
| **Security Audit** | `cargo audit` — CVE scan of the dependency tree | None |

The `Release Build` job uploads the compiled binary as a GitHub Actions
artefact (7-day retention) so you can inspect the exact binary that was built
from any given commit.

---

## Database Migrations

Migrations live in `migrations/` and are embedded into the binary at compile
time via `sqlx::migrate!`. They run automatically every time the server starts
(idempotent — already-applied migrations are skipped).

| Migration | Creates |
|---|---|
| `20250101000001_create_tenants.sql` | `tenants` table, `plan_tier` enum, `set_updated_at()` trigger function |
| `20250101000002_create_users.sql` | `users` table, `user_role` enum, `refresh_tokens` table |
| `20250101000003_create_restaurants.sql` | `restaurants` table |
| `20250101000004_create_menu_items.sql` | `menu_items` table |
| `20250101000005_create_reviews.sql` | `reviews` table, `review_platform` enum |

Manual migration commands:

```sh
export DATABASE_URL=postgres://postgres:password@127.0.0.1:5432/forgebike

sqlx migrate run     # apply pending
sqlx migrate revert  # roll back latest
sqlx migrate info    # show status
```

---

## Roadmap

| Phase | Feature area | Status |
|---|---|---|
| **0** | Project foundations, health endpoint, CI | ✅ Complete |
| **1** | Auth & multi-tenancy (JWT, argon2id, refresh tokens) | ✅ Complete |
| **2** | Restaurant & menu management | ✅ Complete |
| **3** | Review aggregation (Google / Yelp / TripAdvisor) | ✅ Complete |
| **4** | AI sentiment analysis & reply drafts | 🔲 Next |
| **5** | AI marketing content generation (SSE streaming) | 🔲 Planned |
| **6** | Business intelligence & analytics dashboards | 🔲 Planned |
| **7** | Customer engagement — campaigns & AI chat widget | 🔲 Planned |
| **8** | Stripe billing & subscription tier gating | 🔲 Planned |
| **9** | Hardening — OpenTelemetry, RLS, load testing, OpenAPI spec | 🔲 Planned |

Full details for each phase are in [`documentation/architecture.md`](documentation/architecture.md).

---

## Documentation

| Document | Description |
|---|---|
| [`documentation/architecture.md`](documentation/architecture.md) | Full platform design — stack choices, domain model, API surface, all phases |
| [`documentation/phase-0-foundations.md`](documentation/phase-0-foundations.md) | Phase 0 — workspace setup, Docker Compose, health endpoint, CI |
| [`documentation/phase-1-auth.md`](documentation/phase-1-auth.md) | Phase 1 — JWT strategy, Argon2id, token rotation, rate limiting |
| [`documentation/phase-2-restaurants.md`](documentation/phase-2-restaurants.md) | Phase 2 — cursor pagination, PATCH pattern, tenant isolation |
| [`documentation/phase-3-reviews.md`](documentation/phase-3-reviews.md) | Phase 3 — upsert deduplication, descending cursor, external API clients |
