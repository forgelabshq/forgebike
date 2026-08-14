# Phase 6 — Business Intelligence & Analytics

## Overview

Phase 6 adds a real-time analytics layer to the Forgebike platform.  Three
read-only endpoints aggregate KPI data directly from the live `reviews` and
`content_pieces` tables and return JSON responses with a 5-minute Redis cache
to keep database load low.

---

## Architecture

```
HTTP GET ?period=30
        │
        ▼
┌─────────────────────────────────────────────┐
│  API layer (forgebike-api)                  │
│                                             │
│  • Extract AuthIdentity (JWT middleware)    │
│  • Parse & validate ?period param           │
│  • Check Redis cache (5-min TTL)            │
│  • Call AnalyticsService on cache miss      │
│  • Serialise & cache response               │
└──────────────────────┬──────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────┐
│  Application layer (forgebike-application)  │
│                                             │
│  AnalyticsService                           │
│  • Validate period (30 / 90 / 365 only)    │
│  • Verify restaurant belongs to tenant      │
│  • Delegate to AnalyticsRepository port     │
└──────────────────────┬──────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────┐
│  Infrastructure (forgebike-infrastructure)  │
│                                             │
│  PgAnalyticsRepository                      │
│  • Multiple focused SQL aggregation queries │
│  • sqlx::query_as + #[derive(FromRow)]      │
│  • No pre-computed snapshots (real-time)    │
└─────────────────────────────────────────────┘
```

### Design decisions

| Decision | Rationale |
|---|---|
| Real-time SQL aggregation | No background jobs or snapshot tables needed; PostgreSQL handles these queries efficiently with its conditional-aggregate (`FILTER`) syntax |
| Redis cache at handler layer | Keeps `AnalyticsService` pure (easily unit-testable with mock repos); cache is an infrastructure concern, not a business rule |
| 5-minute TTL | Balances freshness against DB load; dashboards typically poll every few minutes |
| `period` whitelist (`30 / 90 / 365`) | Prevents unbounded window sizes; mirrors common SaaS BI product conventions |
| `sqlx::query_as` (not `query!` macros) | Consistent with all other repositories; compiles with `SQLX_OFFLINE=true` in CI without a `.sqlx` cache file |

---

## Endpoints

All endpoints require a valid `Authorization: Bearer <jwt>` header and scope
results to the authenticated tenant.

### `GET /api/v1/restaurants/:id/analytics/overview`

Returns a KPI summary combining review and content statistics.

**Query parameters**

| Parameter | Type | Default | Accepted values |
|---|---|---|---|
| `period` | integer (days) | `30` | `30`, `90`, `365` |

**Response `200 OK`**

```json
{
  "period_days": 30,
  "total_reviews": 142,
  "avg_rating": 4.3,
  "avg_sentiment": 0.71,
  "reviews_with_reply": 38,
  "total_content": 24,
  "published_content": 17
}
```

| Field | Type | Description |
|---|---|---|
| `period_days` | integer | The reporting window echoed back |
| `total_reviews` | integer | Reviews published within the period |
| `avg_rating` | float \| null | Mean star rating (null if no reviews) |
| `avg_sentiment` | float \| null | Mean AI sentiment score (null if none analysed) |
| `reviews_with_reply` | integer | Reviews with a saved AI reply draft |
| `total_content` | integer | Content pieces created within the period |
| `published_content` | integer | Content pieces with `status = published` |

---

### `GET /api/v1/restaurants/:id/analytics/reviews`

Detailed review analytics including distribution breakdowns.

**Response `200 OK`**

```json
{
  "period_days": 30,
  "total_reviews": 142,
  "avg_rating": 4.3,
  "avg_sentiment": 0.71,
  "reviews_with_reply": 38,
  "rating_distribution": { "5": 89, "4": 32, "3": 14, "2": 5, "1": 2 },
  "platform_breakdown": { "google": 98, "yelp": 31, "tripadvisor": 13 }
}
```

| Field | Type | Description |
|---|---|---|
| `rating_distribution` | `{ "1"–"5": integer }` | Count per star rating |
| `platform_breakdown` | `{ platform: integer }` | Count per review platform |

---

### `GET /api/v1/restaurants/:id/analytics/content`

Content-piece analytics broken down by status and type.

**Response `200 OK`**

```json
{
  "period_days": 30,
  "total": 24,
  "by_status": { "draft": 4, "approved": 3, "published": 17 },
  "by_type": { "social_post": 11, "email": 7, "menu_description": 4, "blog_intro": 2 }
}
```

---

## Error responses

| Status | Trigger |
|---|---|
| `401 Unauthorized` | Missing or invalid JWT |
| `404 Not Found` | Restaurant ID does not exist or belongs to another tenant |
| `422 Unprocessable Entity` | `period` is not one of `30`, `90`, `365` |

---

## Caching

Cache keys follow the pattern:

```
analytics:{endpoint}:{tenant_id}:{restaurant_id}:{period_days}
```

Examples:
- `analytics:overview:550e8400-…:f47ac10b-…:30`
- `analytics:reviews:550e8400-…:f47ac10b-…:90`
- `analytics:content:550e8400-…:f47ac10b-…:365`

The tenant ID is embedded in the key, so there is no risk of one tenant
reading another tenant's cached data.  TTL is 300 seconds.

---

## Domain types

The port traits are defined in `forgebike-domain`:

```
crates/domain/src/ports/analytics_port.rs
  ├── AnalyticsRepository (trait)
  ├── OverviewData
  ├── ReviewsAnalyticsData
  └── ContentAnalyticsData
```

---

## SQL queries

All queries follow the pattern used throughout the project: `sqlx::query_as`
with `#[derive(sqlx::FromRow)]` structs.  Key SQL constructs:

```sql
-- Conditional aggregate (PostgreSQL 9.4+)
COUNT(*) FILTER (WHERE ai_reply_draft IS NOT NULL) AS reviews_with_reply

-- Explicit cast to avoid NUMERIC return type from AVG
AVG(rating::DOUBLE PRECISION) AS avg_rating

-- Cast enum to text label
platform::TEXT AS platform
```

---

## Unit tests

`AnalyticsService` is fully unit-tested in
`crates/application/src/analytics/service.rs` using in-memory mock repos:

| Test | Covers |
|---|---|
| `overview_valid_period` | Happy-path overview aggregation |
| `reviews_valid_period` | Happy-path reviews analytics |
| `content_valid_period` | Happy-path content analytics |
| `invalid_period_rejected` | `InvalidPeriod` error returned for period=7 |
| `wrong_tenant_denied` | `RestaurantNotFound` returned for cross-tenant access |

Run with:

```bash
cargo test -p forgebike-application analytics
```

---

## Integration tests

Added to `scripts/test.sh` as `test_phase_6`:

- 401 without auth (all three endpoints)
- 422 for `period=7`, `period=60`, `period=999`
- 200 with correct payload shape for `period=30`, `90`, `365`
- 404 for unknown restaurant (all three endpoints)
- Cross-tenant isolation — other tenant's token cannot see the data (404)

---

## Files added / modified

| File | Change |
|---|---|
| `crates/domain/src/ports/analytics_port.rs` | **New** — port trait + return types |
| `crates/domain/src/ports/mod.rs` | Added `pub mod analytics_port` |
| `crates/infrastructure/src/db/analytics_repository.rs` | **New** — `PgAnalyticsRepository` |
| `crates/infrastructure/src/db/mod.rs` | Re-exported `PgAnalyticsRepository` |
| `crates/application/src/analytics/error.rs` | **New** — `AnalyticsError` |
| `crates/application/src/analytics/service.rs` | **New** — `AnalyticsService` + unit tests |
| `crates/application/src/analytics/mod.rs` | **New** — module declaration |
| `crates/application/src/lib.rs` | Added `pub mod analytics` |
| `crates/api/src/handlers/analytics.rs` | **New** — three handlers with Redis caching |
| `crates/api/src/handlers/mod.rs` | Added `pub mod analytics` |
| `crates/api/src/error.rs` | Added `From<AnalyticsError> for ApiError` |
| `crates/api/src/state.rs` | Added `analytics_service: Arc<AnalyticsService>` |
| `crates/api/src/router.rs` | Wired three analytics routes |
| `crates/server/src/main.rs` | Instantiated repo + service, added to `AppState` |
| `scripts/test.sh` | Added `test_phase_6` function |
| `documentation/phase-6-analytics.md` | **New** — this document |
