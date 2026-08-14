# Restaurant AI Platform — Backend Architecture & Plan of Action

> **Stack**: Rust (backend API) · Python (frontend) · PostgreSQL 16 (primary store) · Redis 7 (cache / sessions)

---

## 1. Product Overview

The platform is a **multi-tenant SaaS** product sold to restaurant owners. Each restaurant
is an isolated tenant with its own data, users, and subscription tier. The backend is
responsible for:

| Domain | Responsibility |
|---|---|
| Auth & Tenancy | Registration, login, JWT sessions, per-tenant isolation |
| Restaurant Profiles | Business info, menus, hours, branding assets |
| Review Management | Aggregation from Google / Yelp / TripAdvisor, AI sentiment scoring, AI reply drafts |
| AI Content Generation | Social posts, email copy, menu descriptions, blog intros — with live SSE streaming |
| Customer Engagement | AI chat widget backend, campaign scheduling, customer segments *(planned)* |
| Business Intelligence | KPI aggregation, competitor snapshots, trend reporting *(planned)* |
| Billing | Subscription tiers, usage metering, Stripe webhooks *(planned)* |

The Python frontend consumes all of this via a versioned **REST + JSON** API (with
Server-Sent Events for streaming content generation, and WebSocket planned for the
live chat widget).

---

## 2. Technology Stack

### Core

| Concern | Crate / Version | Detail |
|---|---|---|
| Web framework | `axum` 0.7 | Built on Tokio + Tower; composable middleware; type-safe extractors |
| Async runtime | `tokio` 1 | De-facto standard; axum requires it |
| Database driver | `sqlx` 0.9 | Fully async; runtime-checked SQL; no compile-time DB connection needed |
| Database | PostgreSQL 16 | `tenant_id` on every table; JSONB; full-text search |
| Cache | Redis 7 via `deadpool-redis` 0.14 | Refresh token store, rate-limit counters, AI token usage counters |
| Serialisation | `serde` 1 + `serde_json` 1 | Universal Rust standard |
| Config | `config` 0.14 + `dotenvy` 0.15 | Layered: `config/default.toml` → `{APP_ENV}.toml` → `APP__*` env vars |
| Logging / Tracing | `tracing` 0.1 + `tracing-subscriber` 0.3 | Structured spans; pretty in dev, JSON in production |
| Error handling | `thiserror` 1 (domain/app) + `anyhow` 1 (server) | Typed errors with ergonomic propagation |
| Migrations | `sqlx migrate` | Embedded, auto-run at startup |

### Auth & Security

| Concern | Crate / Version | Detail |
|---|---|---|
| JWT | `jsonwebtoken` 9 | HS256; 15-min access + 7-day refresh tokens |
| Password hashing | `argon2` 0.5 | Argon2id with random salt |
| Rate limiting | `tower_governor` 0.4 | Per-IP token-bucket on auth routes; configurable burst/rate |
| CORS | `tower-http` 0.5 | `CorsLayer::permissive()` in dev |
| Input validation | `validator` 0.21 | Derive macros; `ValidatedJson<T>` extractor returns 422 on failure |

### AI & External Integrations

| Concern | Crate / Version | Detail |
|---|---|---|
| OpenAI API | `async-openai` 0.28 | Typed async wrapper; sync + streaming (`create_stream`) |
| HTTP client | `reqwest` 0.12 | Used for Google Places, Yelp, TripAdvisor review clients; rustls-TLS |
| Async streams | `futures` 0.3 | `StreamExt` for iterating OpenAI's streaming response |
| SSE streams | `tokio-stream` 0.1 | `UnboundedReceiverStream` converts mpsc channel to SSE |
| Cursor encoding | `base64` 0.22 | URL-safe base64 (no padding) for opaque pagination cursors |

### Developer Tooling

| Tool | Purpose |
|---|---|
| `cargo-watch` | Hot-reload during development |
| `cargo-nextest` | Faster parallel test runner |
| `cargo-audit` | Dependency vulnerability scanning (runs in CI) |
| Docker Compose | Local Postgres 16 (port 5435) + Redis 7 (port 6379) |
| `sqlx-cli` | Running and inspecting migrations from the CLI |
| `scripts/test.sh` | Full-stack dev launcher: starts infra, server, runs curl test suite |

---

## 3. Idiomatic Rust Architecture

The project follows **Hexagonal Architecture** (Ports & Adapters), which maps naturally
to Rust's trait system. Dependencies always point inward — the domain knows nothing about
HTTP, databases, or AI vendors.

```
┌──────────────────────────────────────────────────────────┐
│                        API Layer                         │
│  axum Routers · Handlers · DTOs · Middleware             │
├──────────────────────────────────────────────────────────┤
│                    Application Layer                     │
│  Services (use-case orchestration) · Commands / Queries  │
├──────────────────────────────────────────────────────────┤
│                      Domain Layer                        │
│  Entities · Identifiers · Pagination · Port Traits       │
├──────────────────────┬───────────────────────────────────┤
│  DB Adapters         │  AI Adapter    │  Review Clients  │
│  (sqlx repos)        │  (async-openai)│  (reqwest)       │
└──────────────────────┴───────────────┴──────────────────┘
```

### Key Rust Patterns

**Traits as Ports** — every external dependency is hidden behind a trait so it can be
swapped for an in-memory mock in tests:

```rust
// crates/domain/src/ports/review_repository.rs
#[async_trait]
pub trait ReviewRepository: Send + Sync {
    async fn upsert(&self, review: UpsertReview) -> Result<Review, DomainError>;
    async fn list(&self, tenant_id: TenantId, restaurant_id: RestaurantId, params: ReviewListParams)
        -> Result<Page<Review>, DomainError>;
    async fn find_by_id(&self, tenant_id: TenantId, id: ReviewId)
        -> Result<Option<Review>, DomainError>;
    // ...
}

// crates/domain/src/ports/ai_port.rs
#[async_trait]
pub trait AiContentPort: Send + Sync {
    async fn analyse_sentiment(&self, text: &str)
        -> Result<Option<SentimentResult>, DomainError>;
    async fn generate_reply_draft(&self, context: &ReplyContext)
        -> Result<ReplyDraft, DomainError>;
    async fn generate_content(&self, context: &ContentContext)
        -> Result<ContentDraft, DomainError>;
    async fn stream_content(&self, context: &ContentContext, on_chunk: Arc<dyn Fn(String) + Send + Sync>)
        -> Result<ContentDraft, DomainError>;
}
```

**Newtype Wrappers for IDs** — prevents mixing up `RestaurantId` with `UserId` at compile time:

```rust
// crates/domain/src/identifiers.rs — generated by uuid_id! macro
pub struct TenantId(Uuid);      // newtype; no sqlx::Type — domain stays pure
pub struct RestaurantId(Uuid);
pub struct ReviewId(Uuid);
pub struct ContentPieceId(Uuid);
// ...
```

**State via `Arc<AppState>`** — axum clones cheaply; all fields are `Clone` or `Arc`-wrapped:

```rust
// crates/api/src/state.rs
#[derive(Clone)]
pub struct AppState {
    pub db:                 PgPool,
    pub redis:              deadpool_redis::Pool,
    pub config:             Arc<Config>,
    pub auth_service:       Arc<AuthService>,
    pub restaurant_service: Arc<RestaurantService>,
    pub review_service:     Arc<ReviewService>,
    pub ai_service:         Arc<AiService>,
    pub content_service:    Arc<ContentService>,
}
```

**Layer-scoped error types with `From` + `?`** — each layer defines its own error; the
compiler propagates them via `?` without `.map_err` noise:

```rust
pub enum ContentError {
    RestaurantNotFound(RestaurantId),
    ContentNotFound(ContentPieceId),
    AiUnavailable,
    Domain(#[from] DomainError),   // ← auto-From via thiserror
}

impl From<ContentError> for ApiError { ... }  // maps to HTTP status codes
```

**Descending cursor for newest-first lists** — reviews and content pieces use a
`Cursor::desc_start()` sentinel (year 3000 + max UUID) for the first page, enabling
stable newest-first pagination without `OFFSET`:

```sql
WHERE (created_at, id) < ($cursor_ts, $cursor_id)
ORDER BY created_at DESC, id DESC
LIMIT $n + 1   -- n+1 trick to detect next page without COUNT
```

---

## 4. Cargo Workspace Structure

The project uses a **Cargo workspace** with six crates. Crate boundaries enforce the
hexagonal architecture at the compiler level — the `Cargo.toml` dependency graph makes
it structurally impossible to import a DB function into the domain crate.

```
forgebike/
├── Cargo.toml                      ← workspace manifest (all dep versions pinned here)
├── LICENSE                         ← MIT
├── docker-compose.yml              ← Postgres 16 (5435) + Redis 7 (6379)
├── config/
│   └── default.toml                ← committed baseline config (no secrets)
├── migrations/                     ← sqlx versioned SQL; auto-run at startup
│   ├── 20250101000001_create_tenants.sql
│   ├── 20250101000002_create_users.sql
│   ├── 20250101000003_create_restaurants.sql
│   ├── 20250101000004_create_menu_items.sql
│   ├── 20250101000005_create_reviews.sql
│   └── 20250101000006_create_content_pieces.sql
├── scripts/
│   └── test.sh                     ← full-stack dev launcher + curl test suite
├── documentation/
│   ├── architecture.md             ← this file
│   ├── phase-0-foundations.md
│   ├── phase-1-auth.md
│   ├── phase-2-restaurants.md
│   ├── phase-3-reviews.md
│   ├── phase-4-ai.md
│   └── phase-5-content.md
└── crates/
    ├── config/                     ← typed Config struct; layered loading
    ├── domain/                     ← entities, ID types, port traits, pagination, errors
    ├── application/                ← use-case services (no HTTP / SQL / Redis knowledge)
    │   └── src/
    │       ├── auth/               ← AuthService
    │       ├── restaurant/         ← RestaurantService
    │       ├── review/             ← ReviewService
    │       ├── ai/                 ← AiService (sentiment + reply drafts)
    │       └── content/            ← ContentService (generation + CRUD)
    ├── infrastructure/             ← one crate; adapters for every domain port
    │   └── src/
    │       ├── db/                 ← sqlx repository implementations
    │       ├── redis/              ← RedisTokenStore, RedisTokenUsageStore
    │       ├── ai/                 ← OpenAiClient + prompt templates (include_str!)
    │       └── review_clients/     ← Google, Yelp, TripAdvisor HTTP clients
    ├── api/                        ← axum router, handlers, middleware, extractors, DTOs
    └── server/                     ← binary entry point — composition root only
```

---

## 5. Domain Model (Implemented Entities)

```
Tenant ──< User
       ──< Restaurant ──< MenuItem
                      ──< Review
                      ──< ContentPiece
```

Future entities (`EngagementCampaign`, `CustomerContact`, `AnalyticsSnapshot`) will be
added in Phases 6–7.

### Entity Highlights

| Entity | Key Fields |
|---|---|
| `Tenant` | id, name, plan_tier (`starter`/`growth`/`scale`), stripe_customer_id |
| `User` | id, tenant_id, email, **password_hash** (Argon2id), role (`owner`/`manager`/`viewer`) |
| `Restaurant` | id, tenant_id, name, cuisine_type, address, phone, website, google_place_id, yelp_id |
| `MenuItem` | id, restaurant_id, tenant_id, name, description, **price_cents** (i64), category, is_available |
| `Review` | id, restaurant_id, tenant_id, platform, external_id, author_name, rating, body, published_at, sentiment_score, ai_reply_draft |
| `ContentPiece` | id, restaurant_id, tenant_id, content_type (`social_post`/`email`/`menu_description`/`blog_intro`), title, body, status (`draft`/`approved`/`published`) |

### Pagination Types

All list endpoints use cursor-based pagination implemented in `crates/domain/src/pagination.rs`:

| Type | Purpose |
|---|---|
| `Cursor { created_at, id }` | Position anchor — ascending (`Cursor::start()`) or descending (`Cursor::desc_start()`) |
| `ListParams { limit, cursor }` | Input to every list query |
| `Page<T> { items, next_cursor }` | Output from every list query |

---

## 6. API Surface (Versioned REST)

All routes are prefixed `/api/v1/`. Auth-required endpoints extract `AuthIdentity`
(user_id, tenant_id, role) from the JWT via the `require_auth` middleware.

### Implemented ✅

#### Auth (no auth required — rate-limited)
```
POST   /api/v1/auth/register
POST   /api/v1/auth/login
POST   /api/v1/auth/refresh
POST   /api/v1/auth/logout
GET    /api/v1/auth/me                               ← requires Bearer token
```

#### Restaurants (Bearer required)
```
GET    /api/v1/restaurants                           ← cursor-paginated
POST   /api/v1/restaurants
GET    /api/v1/restaurants/:id
PATCH  /api/v1/restaurants/:id
DELETE /api/v1/restaurants/:id
GET    /api/v1/restaurants/:id/menu                  ← cursor-paginated
POST   /api/v1/restaurants/:id/menu
PATCH  /api/v1/restaurants/:id/menu/:item_id
DELETE /api/v1/restaurants/:id/menu/:item_id
```

#### Reviews (Bearer required)
```
GET    /api/v1/restaurants/:id/reviews               ← cursor-paginated, filterable
POST   /api/v1/restaurants/:id/reviews/sync          ← fetches from Google/Yelp; returns summary
POST   /api/v1/restaurants/:id/reviews/analyse       ← AI sentiment batch; returns count
GET    /api/v1/restaurants/:id/reviews/:rid          ← single review with AI fields
POST   /api/v1/restaurants/:id/reviews/:rid/reply-draft    ← generates + saves AI draft
POST   /api/v1/restaurants/:id/reviews/:rid/reply-publish  ← 501 stub (needs platform OAuth)
```

#### AI Content (Bearer required)
```
POST   /api/v1/restaurants/:id/content/generate      ← sync; returns 201 with draft
GET    /api/v1/restaurants/:id/content/stream        ← SSE; streams tokens live
GET    /api/v1/restaurants/:id/content               ← cursor-paginated, filterable
GET    /api/v1/restaurants/:id/content/:cid
PATCH  /api/v1/restaurants/:id/content/:cid          ← edit body/title/status
DELETE /api/v1/restaurants/:id/content/:cid
```

#### AI Usage (Bearer required)
```
GET    /api/v1/ai/usage                              ← monthly OpenAI token usage for tenant
```

#### Liveness
```
GET    /health                                       ← checks DB + Redis; no auth
```

#### Analytics (Phase 6) ✅
```
GET    /api/v1/restaurants/:id/analytics/overview    ← KPI summary (reviews + content)
GET    /api/v1/restaurants/:id/analytics/reviews     ← rating distribution, platform breakdown
GET    /api/v1/restaurants/:id/analytics/content     ← by status + by type
```

#### Customer Contacts (Phase 7) ✅
```
POST   /api/v1/restaurants/:id/contacts              ← create a contact
GET    /api/v1/restaurants/:id/contacts              ← list (paginated, ?tag= filter)
GET    /api/v1/restaurants/:id/contacts/:cid
PATCH  /api/v1/restaurants/:id/contacts/:cid
DELETE /api/v1/restaurants/:id/contacts/:cid
POST   /api/v1/restaurants/:id/contacts/import       ← bulk JSON import
```

#### Campaigns (Phase 7) ✅
```
POST   /api/v1/restaurants/:id/campaigns
GET    /api/v1/restaurants/:id/campaigns              ← ?status= filter
GET    /api/v1/restaurants/:id/campaigns/:cid
PATCH  /api/v1/restaurants/:id/campaigns/:cid         ← draft only
DELETE /api/v1/restaurants/:id/campaigns/:cid         ← draft only
POST   /api/v1/restaurants/:id/campaigns/:cid/send   ← dispatches via tokio::spawn
```

#### WebSockets (Phase 7) ✅
```
WS     /api/v1/ws/chat/:restaurant_id?token=<jwt>    ← AI chat widget; JWT via query param
```

### Planned 🔲

#### Billing (Phase 8)
```
POST   /api/v1/billing/webhook
```

#### Admin (Phase 8)
```
GET/PATCH  /api/v1/admin/tenants/:id/plan
```

---

## 7. Background Processing

**Current approach (Phases 0–5):** All operations are **synchronous** — the HTTP
handler calls the service, waits for the result, and returns. This keeps the codebase
simple while the number of async use cases is small.

**Phase 7 plan:** When campaigns (email/SMS) and analytics rollups are added, a proper
job queue via **apalis** (Redis backend) will be introduced. The synchronous service
methods will be thin wrappers that enqueue jobs and return immediately.

| Future job | Trigger | Description |
|---|---|---|
| `SyncReviewsJob` | Scheduled (hourly) | Fetches reviews from Google/Yelp, upserts, scores sentiment |
| `SendCampaignJob` | Scheduled (per campaign) | Sends email/SMS via `lettre` / Twilio |
| `ComputeAnalyticsJob` | Scheduled (nightly) | Rolls up KPIs into `AnalyticsSnapshot` |
| `AuditPlanUsageJob` | Scheduled (daily) | Checks per-tenant AI token usage vs. plan limits |

---

## 8. Multi-Tenancy Strategy

Every database table carries a `tenant_id UUID NOT NULL` column. Repository methods
require a `TenantId` argument that can only be produced by the auth middleware after
validating the JWT — callers cannot pass an arbitrary tenant ID.

The service layer enforces a two-step check on every write:
1. **Restaurant ownership** — `RestaurantRepository::find_by_id(tenant_id, id)` returns
   `None` (→ 404) if the restaurant belongs to a different tenant.
2. **Resource ownership** — child resources (reviews, menu items, content pieces) carry
   their own `tenant_id` column, verified in every query.

**Phase 9:** PostgreSQL Row-Level Security policies will be added as a second layer of
defence, enforced at the database level independently of application code.

---

## 9. Plan of Action — Phased Delivery

### Phase 0 — Project Foundations *(Week 1)* ✅

- [x] Convert `forgebike` to a Cargo workspace; create all six `crates/`
- [x] Pin all core dependencies in workspace `Cargo.toml`
- [x] Set up `docker-compose.yml` with Postgres 16 + Redis 7
- [x] Implement `crates/config` — layered loading (`default.toml` → env vars)
- [x] Implement shared error types in `crates/domain`
- [x] Set up `tracing` + `tracing-subscriber` (pretty/JSON by env)
- [x] Migrations: `tenants`, `users` tables
- [x] `GET /health` — returns DB + Redis status
- [x] CI pipeline: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo nextest`, `cargo audit`; fires on every branch push

**Exit criterion**: `cargo run` starts the server; `/health` returns 200; CI is green. ✅

---

### Phase 1 — Auth & Multi-Tenancy *(Week 2)* ✅

- [x] `POST /auth/register` — creates tenant + owner user; Argon2id hash
- [x] `POST /auth/login` — returns HS256 JWT (15 min) + UUID refresh token
- [x] `POST /auth/refresh` — validates Redis-backed refresh token; rotates
- [x] `POST /auth/logout` — revokes refresh token from Redis
- [x] `GET /auth/me` — returns `AuthIdentity` from JWT
- [x] `require_auth` middleware — injects `AuthIdentity` into request extensions
- [x] `RequireOwner` / `RequireManager` role guard extractors
- [x] `ValidatedJson<T>` combined deserialise + validate extractor
- [x] Rate limiting via `tower_governor` — 5-burst/1-per-second per IP on auth routes; configurable via `APP__RATE_LIMIT__*`
- [x] Refresh tokens SHA-256 hashed in Redis (`rt:{hash}` key with TTL)
- [x] Token usage tracking in Redis for AI calls (`ai:tokens:{tenant}:{YYYYMM}`)

**Exit criterion**: A user can register, log in, call a protected endpoint, refresh, and log out. ✅

---

### Phase 2 — Restaurant & Menu Management *(Week 3)* ✅

- [x] Migrations: `restaurants`, `menu_items` tables
- [x] Full CRUD for `Restaurant` — domain entity → sqlx repo → service → handlers
- [x] Full CRUD for `MenuItem` — price stored as `price_cents: i64` (avoids floating point)
- [x] Cursor-based pagination on all list endpoints (ascending `(created_at, id)`)
- [x] `ValidatedJson<T>` validation on all request bodies
- [x] Fetch-then-merge PATCH pattern — field absent in body = keep existing value
- [x] Tenant isolation enforced at service layer (restaurant ownership check before any write)

**Exit criterion**: A tenant can manage multiple restaurants and their menus via the API. ✅

---

### Phase 3 — Review Aggregation *(Weeks 4–5)* ✅

- [x] Migration: `reviews` table with `review_platform` enum
- [x] `crates/infrastructure/src/review_clients/` — async `reqwest` clients:
  - `GooglePlacesClient` (Places Details API)
  - `YelpFusionClient` (Fusion API)
  - `TripAdvisorClient` (Content API — implemented, awaiting column addition)
- [x] `ReviewFetchPort` trait; empty API key → `Ok(vec![])` (graceful skip)
- [x] `POST /reviews/sync` — synchronous; returns `{reviews_synced, platforms_checked, warnings}`
- [x] `GET /reviews` — cursor-paginated descending; filters: `platform`, `min_rating`, `from`, `to`
- [x] Upsert deduplication via `INSERT … ON CONFLICT (restaurant_id, platform, external_id) DO UPDATE`
- [x] Rate-limit overridden in `scripts/test.sh` (`BURST_SIZE=200`) to avoid 429s

**Exit criterion**: Syncing a restaurant fetches reviews from configured platforms. ✅

---

### Phase 4 — AI Sentiment & Reply Drafts *(Week 6)* ✅

- [x] `crates/infrastructure/src/ai/` — `OpenAiClient` implementing `AiContentPort`
- [x] OpenAI key in config; empty key → graceful skip (sentiment) or 503 (reply draft)
- [x] `POST /reviews/analyse` — batch sentiment on reviews with null score; `{analysed, skipped, tokens_used}`
- [x] `GET /reviews/:rid` — single review with AI fields
- [x] `POST /reviews/:rid/reply-draft` — generates + saves draft; 503 if no key, 422 if no body
- [x] `POST /reviews/:rid/reply-publish` — 501 stub (Google/Yelp require partner OAuth)
- [x] Prompt templates as `include_str!` files in `crates/infrastructure/src/ai/prompts/`
- [x] Per-tenant AI token tracking in Redis; recorded but not enforced (enforcement in Phase 8)
- [x] `GET /ai/usage` — returns `{monthly_tokens_used}` for authenticated tenant

**Exit criterion**: A review can be AI-scored for sentiment and an AI reply draft generated. ✅

---

### Phase 5 — AI Content Generation *(Weeks 7–8)* ✅

- [x] Migration: `content_pieces` table with `content_type` and `content_status` enums
- [x] `POST /content/generate` — synchronous; calls OpenAI, saves draft, returns 201
- [x] `GET /content/stream` — SSE; streams tokens via `create_stream`; final event `__done__:<id>`
- [x] `GET /content` — cursor-paginated descending; filters: `status`, `content_type`
- [x] `GET /content/:cid` — single piece
- [x] `PATCH /content/:cid` — partial update (title, body, status)
- [x] `DELETE /content/:cid` — 204 No Content
- [x] Content types: `social_post`, `email` (title = subject), `menu_description`, `blog_intro`
- [x] Single prompt template with `{{CONTENT_TYPE_INSTRUCTION}}` substituted per type
- [x] SSE implemented via `tokio::sync::mpsc::unbounded_channel` + `UnboundedReceiverStream`
- [x] `on_chunk: Arc<dyn Fn(String) + Send + Sync>` callback keeps tokio out of domain layer

**Exit criterion**: A restaurant can generate, stream, edit, approve, and delete AI marketing content. ✅

---

### Phase 6 — Business Intelligence *(Weeks 9–10)* ✅

- [x] `AnalyticsRepository` port trait with three methods (`overview`, `reviews_analytics`, `content_analytics`)
- [x] `PgAnalyticsRepository` — real-time SQL aggregations using `sqlx::query_as` + `#[derive(FromRow)]`
- [x] `AnalyticsService` — validates period (30/90/365), verifies tenant, delegates to port
- [x] `GET /restaurants/:id/analytics/overview` — last 30/90/365 day KPIs
- [x] `GET /restaurants/:id/analytics/reviews` — rating distribution, platform breakdown, avg sentiment
- [x] `GET /restaurants/:id/analytics/content` — totals by status and by content type
- [x] Redis caching (5-min TTL) at the handler layer
- [x] Unit tests for `AnalyticsService` (5 tests: valid periods, invalid period, cross-tenant denial)
- [ ] Competitor snapshot (public review ratings — requires Google Places API, deferred)
- [ ] Pre-computed nightly snapshot table (real-time aggregation sufficient for current scale)

**Exit criterion**: A restaurant owner can view a meaningful analytics dashboard — ✅ met.x

---

### Phase 7 — Customer Engagement *(Weeks 11–12)* ✅

- [x] Migrations: `customer_contacts`, `campaigns` tables
- [x] Campaign CRUD and send (email via lettre, SMS stub)
- [x] Customer contact import (JSON array) and manual add
- [x] Audience segmentation (tag-based)
- [x] AI chat WebSocket endpoint (`WS /api/v1/ws/chat/:restaurant_id`)
  - Stateless per-message: last N turns in context window
  - AI answers questions about the restaurant (hours, menu, reservations)
  - JWT authenticated via `?token=` query parameter
- [ ] apalis (using `tokio::spawn` instead — deferred to Phase 9)
- [ ] Chat session logging (optional, deferred)

**Exit criterion**: A campaign can be sent; the chat widget answers restaurant questions. ✅

---

### Phase 8 — Billing & Subscription *(Week 13)* 🔲 Next

- [ ] `POST /api/v1/billing/webhook` — Stripe webhook; verifies signature; updates tenant plan
- [ ] Plan tiers (`Starter`, `Growth`, `Scale`) gating features in the service layer
- [ ] AI token budget enforcement (uses the Redis counters from Phase 1)
- [ ] `AuditPlanUsageJob` — daily; alerts tenant when approaching AI token cap
- [ ] Admin endpoints for manually adjusting tenant plan

**Exit criterion**: A tenant can be moved between plan tiers and feature access is correctly gated.

---

### Phase 9 — Hardening & Observability *(Week 14+)* 🔲

- [ ] OpenTelemetry export (Jaeger / Honeycomb) via `tracing-opentelemetry`
- [ ] Structured log shipping (stdout JSON → Loki / Datadog)
- [ ] Load testing with `k6` against staging; connection pool tuning
- [ ] PostgreSQL Row-Level Security policies (second layer of tenant isolation)
- [ ] Secrets rotation procedure documented
- [ ] Full API documentation via `utoipa` (generates OpenAPI 3.1 spec for Python frontend)
- [ ] `cargo-deny` for licence + advisory checking in CI

---

## 10. Key Architectural Decisions & Rationale

| Decision | Why |
|---|---|
| `axum` over `actix-web` | Composes cleanly with Tower ecosystem; no actor model; easier to test |
| `sqlx` over `diesel` | Fully async; no ORM overhead; runtime query checking avoids compile-time DB dependency |
| Cargo workspace (6 crates) | Enforces layer boundaries at compiler level; incremental builds |
| Synchronous over async jobs (Phases 0–5) | Keeps the codebase simple while the number of async use cases is small; apalis added in Phase 7 |
| Cursor-based pagination | Stable under concurrent inserts; no OFFSET; O(1) seek cost |
| JWT access + Redis refresh tokens | Short-lived JWTs = no DB on every request; Redis = instant revocation |
| Fetch-then-merge PATCH | Simpler than dynamic SQL; one extra SELECT is acceptable for infrequent updates |
| `on_chunk: Arc<dyn Fn(String) + Send + Sync>` for SSE | Keeps tokio out of domain layer while enabling streaming |
| Prompt templates as `include_str!` files | Versioned in git; editable without recompiling; readable by non-engineers |
| `price_cents: i64` | Avoids floating-point rounding; standard for money in any currency |
| Token refresh rotation | Old token immediately revoked on use; limits replay window to one request |

---

## 11. Environment Variables Reference

All variables use the `APP__SECTION__KEY` double-underscore format. `DATABASE_URL`
(bare, without prefix) is also accepted for tooling compatibility.

```bash
# ── Core ──────────────────────────────────────────────────────
APP_ENV=development                  # development | production
APP__SERVER__HOST=0.0.0.0
APP__SERVER__PORT=8080
APP__APP__LOG_LEVEL=info             # trace | debug | info | warn | error

# ── Database ──────────────────────────────────────────────────
DATABASE_URL=postgres://postgres:password@127.0.0.1:5432/forgebike
APP__DATABASE__MAX_CONNECTIONS=10

# ── Redis ─────────────────────────────────────────────────────
APP__REDIS__URL=redis://127.0.0.1:6379

# ── JWT ───────────────────────────────────────────────────────
APP__JWT__SECRET=<openssl rand -hex 32>   # MUST be changed in production
APP__JWT__ACCESS_TOKEN_EXPIRY_SECS=900    # 15 minutes
APP__JWT__REFRESH_TOKEN_EXPIRY_SECS=604800 # 7 days

# ── Rate limiting ─────────────────────────────────────────────
APP__RATE_LIMIT__BURST_SIZE=5
APP__RATE_LIMIT__PER_SECOND=1

# ── External review APIs (Phase 3) ────────────────────────────
APP__EXTERNAL_APIS__GOOGLE_PLACES_API_KEY=   # empty = skip
APP__EXTERNAL_APIS__YELP_API_KEY=
APP__EXTERNAL_APIS__TRIPADVISOR_API_KEY=

# ── OpenAI (Phases 4–5) ───────────────────────────────────────
APP__AI__OPENAI_API_KEY=             # empty = AI features disabled gracefully
APP__AI__MODEL=gpt-4o-mini
APP__AI__MAX_SENTIMENT_TOKENS=60
APP__AI__MAX_REPLY_TOKENS=300
APP__AI__MAX_CONTENT_TOKENS=600

# ── Future secrets (Phases 7–8) ───────────────────────────────
# APP__STRIPE__SECRET_KEY=
# APP__STRIPE__WEBHOOK_SECRET=
# APP__TWILIO__ACCOUNT_SID=
# APP__TWILIO__AUTH_TOKEN=
```

All secrets are injected at runtime and never committed to source control.
A `.env.example` (no real values) is committed as a template.

---

## 12. What the Python Frontend Receives

The Python frontend only needs to:

1. Call the REST API with an `Authorization: Bearer <access_token>` header
2. Render the JSON responses
3. Subscribe to the SSE endpoint (`GET /content/stream`) for live content generation
4. Connect to the WebSocket endpoint for the AI chat widget *(Phase 7)*

The OpenAPI spec (generated from `utoipa` annotations — Phase 9) will be the formal
contract. Until then, the per-phase documentation files in `documentation/` describe
each endpoint's request/response shape in detail.

---

*Document version: 2.0 — updated to reflect Phases 0–5 implementation.*
