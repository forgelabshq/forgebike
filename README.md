# Forgebike — Restaurant AI Platform

> AI-powered digital growth for restaurants. Automated review management,
> marketing content generation, customer engagement, and business intelligence
> — all delivered through a single multi-tenant API.

[![CI](https://github.com/your-org/forgebike/actions/workflows/ci.yml/badge.svg)](https://github.com/your-org/forgebike/actions/workflows/ci.yml)
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

| Concern | Choice | Why |
|---|---|---|
| Language | Rust (stable) | Performance, memory safety, excellent async story |
| Web framework | `axum` 0.7 | Built on Tokio + Tower; composable middleware; type-safe extractors |
| Async runtime | `tokio` | De-facto standard; required by axum |
| Database | PostgreSQL 16 | Row-level security, JSONB, full-text search |
| DB driver | `sqlx` 0.8 | Fully async; compile-time verified SQL |
| Cache / queues | Redis 7 via `deadpool-redis` | Session store, rate-limit counters, job queues |
| Migrations | `sqlx migrate` | Embedded, versioned SQL files |
| Serialisation | `serde` + `serde_json` | Universal Rust standard |
| Config | `config` + `dotenvy` | Layered: file → env var overrides |
| Tracing | `tracing` + `tracing-subscriber` | Structured spans; pretty in dev, JSON in production |
| Error handling | `thiserror` + `anyhow` | Typed domain errors; ergonomic propagation |

---

## Project Structure

```
forgebike/
├── Cargo.toml                  ← workspace manifest (all dep versions live here)
├── docker-compose.yml          ← local Postgres + Redis
├── config/
│   └── default.toml            ← committed baseline config (no secrets)
├── migrations/                 ← versioned SQL, auto-applied at startup
├── documentation/
│   ├── architecture.md         ← full platform design & phase plan
│   └── phase-0-foundations.md  ← Phase 0 decisions & setup guide
└── crates/
    ├── config/                 ← layered config loading
    ├── domain/                 ← entities, ID types, error types, port traits
    ├── api/                    ← axum router, handlers, middleware, DTOs
    └── server/                 ← binary entry point — wires everything together
```

### Crate dependency graph

```
forgebike-server  (binary)
    ├── forgebike-api
    │       ├── forgebike-config
    │       └── forgebike-domain
    └── forgebike-config

forgebike-domain  ← zero internal deps (pure Rust + serde + uuid)
```

Dependencies always point **inward**. The domain layer has no knowledge of
HTTP, databases, or any external service. Architecture boundaries are enforced
at the compiler level — you cannot import a DB function into the domain crate
because there is no declared dependency between them.

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
cargo install cargo-nextest  # faster test runner
```

### 1. Configure environment

```sh
cp .env.example .env
```

The defaults in `.env.example` work out of the box with the Docker Compose
stack. If you have a native PostgreSQL instance already running on port 5432,
use `127.0.0.1` (not `localhost`) in your connection string to force TCP auth:

```sh
# Native postgres on 5432
DATABASE_URL=postgres://postgres:password@127.0.0.1:5432/forgebike

# Docker postgres (mapped to 5435 to avoid port conflicts)
DATABASE_URL=postgres://postgres:password@127.0.0.1:5435/forgebike
```

### 2. Start infrastructure

```sh
docker compose up -d
```

This starts:
- **PostgreSQL 16** on host port `5435` (dev) and `5436` (test)
- **Redis 7** on host port `6379`

> If you already have a native PostgreSQL running, the Docker postgres
> services use ports `5435`/`5436` to avoid conflicts.

### 3. Run the server

```sh
cargo run --bin forgebike
```

Database migrations run automatically at startup. The server listens on
`http://0.0.0.0:8080`.

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

A `503 Service Unavailable` with `"status": "degraded"` means one of the
infrastructure components is unreachable.

### Hot reload

```sh
cargo watch -x 'run --bin forgebike'
```

---

## Configuration

Config is loaded in layers — each layer overrides the one before it:

| Priority | Source | Committed? |
|---|---|---|
| 1 (lowest) | `config/default.toml` | Yes — no secrets |
| 2 | `config/{APP_ENV}.toml` | No — local overrides, gitignored |
| 3 (highest) | Environment variables (`APP__*`) | Via platform or `.env` |

### Key variables

| Variable | Default | Description |
|---|---|---|
| `APP_ENV` | `development` | Switches log format (pretty/JSON) and per-env config file |
| `DATABASE_URL` | see `default.toml` | Postgres connection string — also accepts `APP__DATABASE__URL` |
| `APP__DATABASE__MAX_CONNECTIONS` | `10` | PgPool size |
| `APP__REDIS__URL` | `redis://127.0.0.1:6379` | Redis connection string |
| `APP__SERVER__PORT` | `8080` | HTTP bind port |
| `APP__APP__LOG_LEVEL` | `info` | `trace` · `debug` · `info` · `warn` · `error` |
| `RUST_LOG` | — | Fallback log filter if `APP__APP__LOG_LEVEL` is not set |

Secrets for upcoming phases (`JWT_SECRET`, `OPENAI_API_KEY`, etc.) are
documented in `.env.example` — they are commented out until the relevant
phase is implemented.

---

## API Endpoints

### Currently implemented (Phase 0)

| Method | Path | Auth | Description |
|---|---|---|---|
| `GET` | `/health` | None | Liveness probe — checks DB and Redis |

### Planned (see `documentation/architecture.md` for the full surface)

```
POST   /api/v1/auth/register
POST   /api/v1/auth/login
POST   /api/v1/auth/refresh
POST   /api/v1/auth/logout

GET    /api/v1/restaurants
POST   /api/v1/restaurants
GET    /api/v1/restaurants/:id
PATCH  /api/v1/restaurants/:id

GET    /api/v1/restaurants/:id/reviews
POST   /api/v1/restaurants/:id/reviews/sync
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
# Fast parallel test runner
cargo nextest run

# Or the built-in runner
cargo test
```

Integration tests require the Docker stack to be running (`docker compose up -d`).

### Linting and formatting

```sh
cargo fmt --all           # format
cargo fmt --all --check   # check (used in CI)
cargo clippy --all-targets
```

Clippy runs with `pedantic` warnings enabled. `unsafe_code` is **forbidden**
across the entire workspace.

### Adding a migration

```sh
sqlx migrate add <migration_name>
# Edit the generated file in migrations/
# Migrations run automatically on next cargo run
```

### Checking migration status

```sh
sqlx migrate info --database-url postgres://postgres:password@127.0.0.1:5432/forgebike
```

---

## CI Pipeline

Three jobs run on every push and pull request:

| Job | What it checks | DB needed? |
|---|---|---|
| **Check & Lint** | `cargo fmt`, `cargo clippy`, `cargo check` | No (`SQLX_OFFLINE=true`) |
| **Tests** | `cargo nextest` against real Postgres + Redis | Yes (GitHub Actions services) |
| **Security Audit** | `cargo audit` — CVE scan of the dependency tree | No |

In-progress runs for the same branch are automatically cancelled.

---

## Database Migrations

Migrations live in `migrations/` and are embedded into the binary at compile
time. They run automatically every time the server starts (idempotent — already
applied migrations are skipped).

| Migration | Creates |
|---|---|
| `20250101000001_create_tenants.sql` | `tenants` table, `plan_tier` enum, `set_updated_at()` trigger |
| `20250101000002_create_users.sql` | `users` table, `user_role` enum, `refresh_tokens` table |

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
| **1** | Auth & multi-tenancy (JWT, argon2id, refresh tokens) | 🔲 Next |
| **2** | Restaurant & menu management | 🔲 Planned |
| **3** | Review aggregation (Google / Yelp / TripAdvisor) | 🔲 Planned |
| **4** | AI sentiment analysis & reply drafts | 🔲 Planned |
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
| [`documentation/phase-0-foundations.md`](documentation/phase-0-foundations.md) | Phase 0 deep-dive — setup guide, config reference, CI explanation |
