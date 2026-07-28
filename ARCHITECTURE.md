# Restaurant AI Platform — Backend Architecture & Plan of Action

> **Stack**: Rust (backend API) · Python (frontend) · PostgreSQL (primary store) · Redis (cache / queues)

---

## 1. Product Overview

The platform is a **multi-tenant SaaS** product sold to restaurant owners. Each restaurant
is an independent tenant with its own data, users, and subscription tier. The backend is
responsible for:

| Domain | Responsibility |
|---|---|
| Auth & Tenancy | Registration, login, JWT sessions, per-tenant isolation |
| Restaurant Profiles | Business info, menus, hours, branding assets |
| AI Content Generation | Social posts, email copy, blog articles, menu descriptions |
| Review Management | Aggregation from Google / Yelp / TripAdvisor, AI reply drafts, sentiment |
| Customer Engagement | AI chat widget backend, campaign scheduling, customer segments |
| Business Intelligence | KPI aggregation, competitor snapshots, trend reporting |
| Billing | Subscription tiers, usage metering, Stripe webhooks |

The Python frontend consumes all of this via a versioned **REST + JSON** API (with
WebSocket support for live engagement features).

---

## 2. Technology Stack

### Core

| Concern | Crate / Tool | Rationale |
|---|---|---|
| Web framework | `axum` | Built on Tokio + Tower; composable middleware; first-class `async`; excellent ecosystem growth |
| Async runtime | `tokio` | De-facto standard; axum requires it |
| Database driver | `sqlx` | Compile-time SQL verification; fully async; no macro magic ORM overhead; supports Postgres |
| Database | PostgreSQL 16 | Row-level security for multi-tenancy; JSONB for flexible AI payloads; full-text search |
| Cache / Queue | Redis (via `redis` + `deadpool-redis`) | Session caching, rate-limit counters, background job queues |
| Serialisation | `serde` + `serde_json` | Universal Rust standard |
| Config | `config` + `dotenvy` | Layered config (file → env var override); `.env` for local dev |
| Logging / Tracing | `tracing` + `tracing-subscriber` | Structured spans; integrates with axum via `tower-http` |
| Error handling | `thiserror` (domain) + `anyhow` (application glue) | `thiserror` for typed errors in library code; `anyhow` for handler convenience |
| Migrations | `sqlx migrate` | Embedded, versioned SQL migrations checked into source control |

### Auth & Security

| Concern | Crate | Notes |
|---|---|---|
| JWT | `jsonwebtoken` | Access + refresh token pair |
| Password hashing | `argon2` | Argon2id; never bcrypt for new projects |
| Rate limiting | `tower_governor` | Tower middleware layer; per-IP and per-tenant |
| CORS | `tower-http` | Built-in `CorsLayer` |
| Input validation | `validator` | Derive-macro validation on request DTOs |

### AI & External Integrations

| Concern | Crate / Approach | Notes |
|---|---|---|
| OpenAI API | `async-openai` | Typed, async wrapper; supports streaming responses |
| HTTP client | `reqwest` | Async; used for Google / Yelp / TripAdvisor API calls |
| Background jobs | `apalis` (Redis backend) | Typed job queues; retries; scheduling |
| Email | `lettre` | SMTP or API-based transactional email |
| Stripe webhooks | `stripe-rust` or raw `reqwest` | Billing events |

### Developer Tooling

| Tool | Purpose |
|---|---|
| `cargo-watch` | Hot-reload during development |
| `cargo-nextest` | Faster test runner |
| `cargo-audit` | Dependency vulnerability scanning |
| Docker + `docker-compose` | Local Postgres + Redis |
| `sqlx-cli` | Running and creating migrations from the CLI |

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
│  Services (use-case orchestration) · Command / Query     │
├──────────────────────────────────────────────────────────┤
│                      Domain Layer                        │
│  Entities · Value Objects · Domain Errors · Port Traits  │
├────────────────────┬─────────────────────────────────────┤
│   DB Adapters      │  AI Adapters   │  External Adapters │
│  (sqlx repos)      │  (async-openai)│  (reqwest clients) │
└────────────────────┴─────────────────────────────────────┘
```

### Key Rust Patterns

**Traits as Ports** — every external dependency is hidden behind a trait so it can be
swapped for a mock in tests:

```forgebike/src/domain/ports.rs#L1-20
pub trait ReviewRepository: Send + Sync {
    async fn find_by_restaurant(
        &self,
        tenant_id: TenantId,
        restaurant_id: RestaurantId,
    ) -> Result<Vec<Review>, DomainError>;
}

pub trait AiContentPort: Send + Sync {
    async fn generate_social_post(
        &self,
        context: &ContentContext,
    ) -> Result<GeneratedContent, DomainError>;
}
```

**Newtype Wrappers for IDs** — prevents mixing up `RestaurantId` with `UserId`:

```forgebike/src/domain/identifiers.rs#L1-10
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(transparent)]
pub struct TenantId(uuid::Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(transparent)]
pub struct RestaurantId(uuid::Uuid);
```

**State via `Arc<AppState>`** — axum extracts a shared, cheaply-cloned state object:

```forgebike/src/api/state.rs#L1-15
#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool,
    pub redis: deadpool_redis::Pool,
    pub config: Arc<Config>,
    pub ai: Arc<dyn AiContentPort>,
    pub review_fetcher: Arc<dyn ReviewFetchPort>,
}
```

**`From` + `?` for error propagation** — each layer defines its own error type and
implements `From<LowerError>` so `?` works cleanly across boundaries without `.map_err`
everywhere.

---

## 4. Cargo Workspace Structure

The project uses a **Cargo workspace** to separate concerns into crates that can be
compiled and tested independently, and to keep compile times manageable as the codebase
grows.

```
forgebike/
├── Cargo.toml                  ← workspace manifest
├── ARCHITECTURE.md             ← this document
│
├── crates/
│   ├── api/                    ← axum HTTP layer (routes, handlers, DTOs, middleware)
│   ├── application/            ← use-case services (orchestrate domain + ports)
│   ├── domain/                 ← entities, value objects, port traits, domain errors
│   ├── infrastructure/
│   │   ├── db/                 ← sqlx repository implementations
│   │   ├── ai/                 ← OpenAI adapter
│   │   ├── review_clients/     ← Google / Yelp / TripAdvisor HTTP clients
│   │   ├── email/              ← lettre adapter
│   │   └── billing/            ← Stripe adapter
│   ├── jobs/                   ← apalis background job definitions and workers
│   └── config/                 ← shared Config struct, loaded once at startup
│
├── migrations/                 ← sqlx versioned SQL migrations
│   ├── 0001_tenants.sql
│   ├── 0002_users.sql
│   ├── 0003_restaurants.sql
│   └── ...
│
└── tests/
    └── integration/            ← full-stack integration tests against a real PG instance
```

---

## 5. Domain Model (Core Entities)

```
Tenant ──< Restaurant ──< Review
                      ──< MenuItem
                      ──< ContentPiece
                      ──< EngagementCampaign
                      ──< AnalyticsSnapshot

User >── TenantMembership ──< Tenant
```

### Entity Highlights

| Entity | Key Fields |
|---|---|
| `Tenant` | id, name, plan_tier, stripe_customer_id, created_at |
| `User` | id, tenant_id, email, argon2_hash, role (Owner / Manager / Viewer) |
| `Restaurant` | id, tenant_id, name, cuisine_type, address, google_place_id, yelp_id |
| `MenuItem` | id, restaurant_id, name, description, price, category, ai_generated_description |
| `Review` | id, restaurant_id, platform, external_id, rating, body, sentiment_score, ai_reply_draft |
| `ContentPiece` | id, restaurant_id, content_type (social/email/blog), body, status (draft/approved/published) |
| `EngagementCampaign` | id, restaurant_id, channel (email/sms), schedule, target_segment, ai_copy |
| `AnalyticsSnapshot` | id, restaurant_id, period, avg_rating, review_count, sentiment_avg, content_published |

---

## 6. API Surface (Versioned REST)

All routes are prefixed `/api/v1/`. Auth-required endpoints extract the tenant context
from the JWT claim — there is no way to access another tenant's data.

### Auth
```
POST   /api/v1/auth/register
POST   /api/v1/auth/login
POST   /api/v1/auth/refresh
POST   /api/v1/auth/logout
```

### Restaurants
```
GET    /api/v1/restaurants
POST   /api/v1/restaurants
GET    /api/v1/restaurants/:id
PATCH  /api/v1/restaurants/:id
DELETE /api/v1/restaurants/:id
GET    /api/v1/restaurants/:id/menu
POST   /api/v1/restaurants/:id/menu
PATCH  /api/v1/restaurants/:id/menu/:item_id
```

### Reviews
```
GET    /api/v1/restaurants/:id/reviews          ← aggregated, paginated
POST   /api/v1/restaurants/:id/reviews/sync     ← trigger external fetch job
GET    /api/v1/restaurants/:id/reviews/:rid
POST   /api/v1/restaurants/:id/reviews/:rid/reply-draft   ← AI draft
POST   /api/v1/restaurants/:id/reviews/:rid/reply-publish ← publish via platform API
```

### AI Content
```
POST   /api/v1/restaurants/:id/content/generate   ← fire generation job
GET    /api/v1/restaurants/:id/content            ← list pieces
GET    /api/v1/restaurants/:id/content/:cid
PATCH  /api/v1/restaurants/:id/content/:cid       ← edit / approve
DELETE /api/v1/restaurants/:id/content/:cid
```

### Campaigns
```
GET    /api/v1/restaurants/:id/campaigns
POST   /api/v1/restaurants/:id/campaigns
GET    /api/v1/restaurants/:id/campaigns/:cid
PATCH  /api/v1/restaurants/:id/campaigns/:cid
DELETE /api/v1/restaurants/:id/campaigns/:cid
POST   /api/v1/restaurants/:id/campaigns/:cid/send
```

### Analytics
```
GET    /api/v1/restaurants/:id/analytics/overview
GET    /api/v1/restaurants/:id/analytics/reviews
GET    /api/v1/restaurants/:id/analytics/content
GET    /api/v1/restaurants/:id/analytics/engagement
```

### Admin (internal, separate auth role)
```
GET    /api/v1/admin/tenants
GET    /api/v1/admin/tenants/:id
PATCH  /api/v1/admin/tenants/:id/plan
GET    /api/v1/admin/jobs
```

### WebSockets
```
WS     /api/v1/ws/chat/:restaurant_id   ← AI chat widget for restaurant's customers
```

---

## 7. Background Jobs (apalis)

Long-running or async tasks are decoupled from the HTTP request cycle using **apalis**
with a Redis backend. This prevents API timeouts and allows retrying failed jobs.

| Job | Trigger | Description |
|---|---|---|
| `SyncReviewsJob` | Scheduled (hourly) + manual API call | Fetches new reviews from Google / Yelp, stores, runs sentiment |
| `GenerateContentJob` | API request | Calls OpenAI to produce a content piece, stores draft |
| `PublishReplyJob` | API request | Posts approved reply to Google / Yelp via their write APIs |
| `SendCampaignJob` | Scheduled (per campaign) | Sends email/SMS campaign via `lettre` / Twilio |
| `ComputeAnalyticsJob` | Scheduled (nightly) | Rolls up KPIs into `AnalyticsSnapshot` for fast dashboard queries |
| `AuditPlanUsageJob` | Scheduled (daily) | Checks per-tenant AI token usage against plan limits; alerts if near cap |

---

## 8. Multi-Tenancy Strategy

Every database table includes a `tenant_id UUID NOT NULL` column. The sqlx repositories
**always** filter by the `tenant_id` extracted from the authenticated JWT — never by a
user-supplied parameter alone. This is enforced at the Rust type level: repository
methods require a `TenantId` argument that can only be produced by the auth middleware.

For stricter isolation in a later phase, PostgreSQL **Row-Level Security (RLS)** can be
enabled per table, with the Rust connection pool setting `app.current_tenant` as a
session variable that RLS policies read. This provides a database-enforced second layer
of defence.

---

## 9. Plan of Action — Phased Delivery

### Phase 0 — Project Foundations *(Week 1)*

This is the skeleton everything else is built on. Nothing else can start until this is
done.

- [ ] Convert `forgebike` to a Cargo workspace; create `crates/` sub-crates
- [ ] Add all core dependencies to the workspace `Cargo.toml`
- [ ] Set up `docker-compose.yml` with Postgres 16 + Redis
- [ ] Implement `crates/config` — loads from `config/default.toml` + env vars
- [ ] Implement shared error types in `crates/domain`
- [ ] Set up `tracing` + `tracing-subscriber` with JSON output for production
- [ ] Write migration `0001_tenants.sql` and `0002_users.sql`
- [ ] Implement `GET /health` endpoint — returns db + redis connectivity status
- [ ] Set up `tests/integration/` harness (spins up test DB, runs migrations)
- [ ] CI pipeline (GitHub Actions): `cargo fmt --check`, `cargo clippy`, `cargo nextest`

**Exit criterion**: `cargo run` starts the server; `/health` returns 200; CI is green.

---

### Phase 1 — Auth & Multi-Tenancy *(Week 2)*

Every other feature depends on knowing who the request is from and which tenant it
belongs to.

- [ ] Migration: `users`, `tenant_memberships`, `refresh_tokens` tables
- [ ] `POST /auth/register` — creates tenant + owner user; argon2id hash
- [ ] `POST /auth/login` — returns signed JWT access token (15 min) + refresh token
- [ ] `POST /auth/refresh` — validates refresh token from Redis; issues new pair
- [ ] `POST /auth/logout` — revokes refresh token in Redis
- [ ] `AuthLayer` axum middleware — validates JWT, injects `TenantId` + `UserId` into request extensions
- [ ] Role-based guard extractor (`RequireRole<Owner>`, `RequireRole<Manager>`)
- [ ] Rate limit middleware via `tower_governor` (100 req/min per IP; 10 req/min on auth routes)
- [ ] Integration tests for all auth flows

**Exit criterion**: A user can register, log in, call a protected endpoint, refresh, and log out.

---

### Phase 2 — Restaurant & Menu Management *(Week 3)*

Core CRUD that all other features are anchored to.

- [ ] Migrations: `restaurants`, `menu_items` tables
- [ ] Full CRUD for `Restaurant` (domain entity → sqlx repo → application service → axum handlers)
- [ ] Full CRUD for `MenuItem`
- [ ] Input validation with `validator` derive macros on all request DTOs
- [ ] Pagination helper (cursor-based) shared across all list endpoints
- [ ] Integration tests for restaurant and menu endpoints

**Exit criterion**: A tenant can manage multiple restaurants and their menus via the API.

---

### Phase 3 — Review Aggregation *(Weeks 4–5)*

Pulling in reviews from external platforms is the first AI-adjacent feature and lays the
groundwork for both sentiment analysis and response generation.

- [ ] Migration: `reviews` table
- [ ] `crates/infrastructure/review_clients` — async `reqwest`-based clients:
  - Google Places API (reviews endpoint)
  - Yelp Fusion API
  - TripAdvisor Content API
- [ ] `ReviewFetchPort` trait in domain; adapters in infrastructure
- [ ] `SyncReviewsJob` in `crates/jobs` — fetches, deduplicates (by `external_id`), stores
- [ ] `POST /restaurants/:id/reviews/sync` — enqueues the job; returns 202 Accepted
- [ ] `GET /restaurants/:id/reviews` — paginated, filterable by platform / rating / date
- [ ] Integration tests (mock HTTP server for external APIs)

**Exit criterion**: Syncing a restaurant pulls real reviews from at least one platform.

---

### Phase 4 — AI Sentiment & Reply Drafts *(Week 6)*

This is the first visible AI feature to end users.

- [ ] `crates/infrastructure/ai` — `async-openai` adapter implementing `AiContentPort`
- [ ] OpenAI API key stored in config / environment (never in source)
- [ ] Sentiment scoring on review ingest (score stored on `Review.sentiment_score`)
- [ ] `POST /restaurants/:id/reviews/:rid/reply-draft` — calls AI with review context, business tone, returns draft
- [ ] `POST /restaurants/:id/reviews/:rid/reply-publish` — posts approved reply via platform API
- [ ] Prompt templates stored as versioned files in `crates/ai/prompts/` (not hardcoded)
- [ ] Per-tenant AI token usage tracking; stored in Redis; enforced against plan limits
- [ ] Unit tests for prompt construction; integration test for full reply draft flow

**Exit criterion**: A review can be fetched, AI-scored for sentiment, and an AI reply draft generated.

---

### Phase 5 — AI Content Generation *(Weeks 7–8)*

The marketing content engine — the most frequently used AI feature.

- [ ] Migration: `content_pieces` table
- [ ] `GenerateContentJob` — takes content type + restaurant context, calls AI, stores draft
- [ ] `POST /restaurants/:id/content/generate` — enqueues job; returns 202 with job ID
- [ ] `GET /restaurants/:id/content` — list with status filter (draft / approved / published)
- [ ] `PATCH /restaurants/:id/content/:cid` — edit copy, change status
- [ ] Content types supported: social post (Twitter/Instagram/Facebook), email subject+body, menu item description, blog intro
- [ ] Streaming response endpoint (`GET /api/v1/content/stream/:job_id`) via SSE for live generation preview
- [ ] Integration tests for content generation pipeline

**Exit criterion**: A restaurant can generate, review, edit, and approve AI marketing content.

---

### Phase 6 — Business Intelligence *(Weeks 9–10)*

Aggregated reporting that demonstrates measurable value to the client.

- [ ] Migration: `analytics_snapshots` table
- [ ] `ComputeAnalyticsJob` — nightly rollup; calculates avg rating, sentiment trend, content velocity, review response rate
- [ ] `GET /restaurants/:id/analytics/overview` — last 30 / 90 / 365 day KPIs
- [ ] `GET /restaurants/:id/analytics/reviews` — rating trend, platform breakdown, sentiment chart data
- [ ] `GET /restaurants/:id/analytics/content` — published vs draft ratio, top performing content types
- [ ] Competitor snapshot (if Google place IDs supplied): pull their public review ratings for comparison
- [ ] Response caching in Redis for analytics endpoints (5-min TTL)
- [ ] Integration tests for analytics rollup job

**Exit criterion**: A restaurant owner can view a meaningful analytics dashboard with at least 30 days of data.

---

### Phase 7 — Customer Engagement *(Weeks 11–12)*

Campaigns and the live AI chat widget.

- [ ] Migration: `engagement_campaigns`, `customer_contacts` tables
- [ ] Campaign CRUD and `SendCampaignJob` (email via `lettre`; SMS via Twilio `reqwest` client)
- [ ] Customer contact import (CSV) and manual add
- [ ] Audience segmentation (by last visit date, review left, loyalty tier)
- [ ] AI chat WebSocket endpoint (`WS /api/v1/ws/chat/:restaurant_id`)
  - Stateless: each message includes last N turns in context window
  - AI answers questions about the restaurant (hours, menu, reservations)
  - Configurable persona per restaurant
- [ ] Chat session logging (optional, consent-gated)
- [ ] Integration tests for campaign send and chat session

**Exit criterion**: A campaign can be composed, scheduled, and sent; the chat widget can answer questions about a restaurant.

---

### Phase 8 — Billing & Subscription *(Week 13)*

Monetisation layer — gating features by plan tier.

- [ ] `POST /api/v1/billing/webhook` — Stripe webhook handler (signature verified); updates tenant plan on events
- [ ] Plan tiers defined in config: `Starter`, `Growth`, `Scale`
- [ ] Feature flags per plan checked in application service layer (not in handlers)
- [ ] `AuditPlanUsageJob` — daily check; emails tenant if near AI token cap
- [ ] Admin endpoints for manually adjusting tenant plan

**Exit criterion**: A tenant can be moved between plan tiers and feature access is correctly gated.

---

### Phase 9 — Hardening & Observability *(Week 14+)*

- [ ] OpenTelemetry export (Jaeger / Honeycomb) via `tracing-opentelemetry`
- [ ] Structured log shipping (stdout JSON → Loki / Datadog)
- [ ] `cargo-audit` in CI; Dependabot for automated dep updates
- [ ] Load testing with `k6` against staging
- [ ] Database connection pool tuning; slow query logging
- [ ] Secrets rotation procedure documented
- [ ] RLS policies enabled and tested for multi-tenant isolation
- [ ] Full API documentation via `utoipa` (generates OpenAPI 3.1 spec consumed by Python frontend)

---

## 10. Key Architectural Decisions & Rationale

| Decision | Why |
|---|---|
| `axum` over `actix-web` | `actix-web` uses its own runtime and actor model; `axum` composes cleanly with the wider Tower/Tokio ecosystem and is easier to test |
| `sqlx` over `diesel` | `diesel` is synchronous (requires connection-per-thread), which fights against async axum. `sqlx` is fully async and its compile-time query checking gives similar safety guarantees |
| Cargo workspace (multiple crates) | Keeps compile units small; enforces architectural boundaries at the Rust module system level (you cannot accidentally call a DB function from the domain layer if it is in a different crate with no declared dependency) |
| Background jobs via `apalis` | Keeps API response times predictable; decouples retry logic from HTTP handlers; allows job observability |
| Prompt templates as files | Prompts are iterated on rapidly; storing them as files (not Rust string literals) means non-engineers can tweak them without recompiling |
| Cursor-based pagination | Offset pagination is slow on large tables and inconsistent when rows are inserted during traversal; cursor-based is correct and performant |
| JWT access + Redis refresh tokens | Short-lived JWTs reduce DB lookups; Redis-backed refresh tokens allow instant revocation without a DB row-lock per request |

---

## 11. Environment Variables Reference

```
DATABASE_URL=postgres://user:pass@localhost:5432/forgebike
REDIS_URL=redis://localhost:6379
JWT_SECRET=<256-bit random secret>
OPENAI_API_KEY=<key>
GOOGLE_PLACES_API_KEY=<key>
YELP_API_KEY=<key>
STRIPE_SECRET_KEY=<key>
STRIPE_WEBHOOK_SECRET=<key>
TWILIO_ACCOUNT_SID=<sid>
TWILIO_AUTH_TOKEN=<token>
APP_ENV=development|production
LOG_LEVEL=info
```

All secrets are injected via environment variables at runtime. They are **never**
committed to source control. A `.env.example` file (no real values) is committed as a
template.

---

## 12. What the Python Frontend Receives

The Python frontend only needs to:

1. Call the REST API with a Bearer token
2. Render the JSON responses
3. Connect to the WebSocket endpoint for chat

The OpenAPI spec (generated from `utoipa` annotations) will be the contract. This should
be generated and published as part of the CI pipeline so the frontend team always has
an up-to-date spec to code against.

---

*Document version: 1.0 — to be updated as decisions are made during implementation.*
