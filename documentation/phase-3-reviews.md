# Phase 3 — Review Aggregation

> **Status**: Complete  
> **Timeframe**: Weeks 4–5  
> **Exit criterion**: `POST /reviews/sync` fetches reviews from configured platforms; `GET /reviews` returns a filtered, cursor-paginated list; tenant isolation is enforced; 101/101 tests pass.

---

## What Was Built

| Deliverable | Location |
|---|---|
| Migration: `reviews` table | `migrations/20250101000005_create_reviews.sql` |
| `Review` entity + `ReviewPlatform` enum | `crates/domain/src/entities/review.rs` |
| `Cursor::desc_start()` — descending pagination sentinel | `crates/domain/src/pagination.rs` |
| `ReviewFetchPort` port trait + `FetchedReview` | `crates/domain/src/ports/review_fetch_port.rs` |
| `ReviewRepository` port trait + `UpsertReview`, `ReviewListParams` | `crates/domain/src/ports/review_repository.rs` |
| `ReviewService` — sync + list use cases | `crates/application/src/review/service.rs` |
| `PgReviewRepository` — upsert + descending paginated list | `crates/infrastructure/src/db/review_repository.rs` |
| `GooglePlacesClient` — Places Details API | `crates/infrastructure/src/review_clients/google.rs` |
| `YelpFusionClient` — Yelp Fusion API | `crates/infrastructure/src/review_clients/yelp.rs` |
| `TripAdvisorClient` — Content API (implemented, not yet wired) | `crates/infrastructure/src/review_clients/tripadvisor.rs` |
| `ExternalApisConfig` added to `Config` | `crates/config/src/lib.rs` |
| Two new REST handlers | `crates/api/src/handlers/reviews.rs` |
| Phase 3 tests (16 new curl assertions) | `scripts/test.sh` |
| 7 new unit tests in `review::service::tests` | `crates/application/src/review/service.rs` |

---

## Architecture Decisions

### Synchronous Sync (no job queue yet)

The architecture document plans `apalis` for background jobs, but adding a
full Redis-backed job queue for a single use case is premature. Phase 3
implements `POST /reviews/sync` as a synchronous endpoint: the handler
calls `ReviewService::sync_reviews`, waits for all platform fetches to
complete, then returns the summary.

**Trade-off**: a slow external API (e.g. Yelp responding in 3 s) will hold
the HTTP connection open. In practice this is fine for a management endpoint
called infrequently by an authenticated owner. Apalis will be added in Phase 7
when multiple job types justify the infrastructure.

### Upsert Deduplication

Reviews are deduplicated by the triple `(restaurant_id, platform, external_id)`.
The PostgreSQL `INSERT … ON CONFLICT … DO UPDATE` pattern means re-syncing
the same reviews is safe — it updates the author name, rating, body, and
`published_at` if the review was edited, without creating duplicates.

```sql
INSERT INTO reviews (restaurant_id, tenant_id, platform, external_id, …)
VALUES (…)
ON CONFLICT (restaurant_id, platform, external_id) DO UPDATE
    SET author_name  = EXCLUDED.author_name,
        rating       = EXCLUDED.rating,
        body         = EXCLUDED.body,
        published_at = EXCLUDED.published_at
RETURNING …;
```

### Descending Cursor Pagination

Restaurant and menu item lists use **ascending** pagination
(`ORDER BY created_at ASC`) with `Cursor::start()` (epoch + nil UUID) as
the first-page sentinel.

Review lists use **descending** pagination (`ORDER BY published_at DESC`)
because callers expect newest-first ordering. A new `Cursor::desc_start()`
sentinel (year 3000 + max UUID) makes `(published_at, id) < desc_start()`
always true for the first page, so the same SQL query pattern works without
any conditional logic.

```sql
WHERE …
  AND (published_at < $cursor_ts
       OR (published_at = $cursor_ts AND id < $cursor_id))
ORDER BY published_at DESC, id DESC
LIMIT $n + 1
```

The `Cursor` type is reused between ascending and descending contexts — the
`created_at` field represents whichever timestamp the sort is anchored on.

### Optional Filter Pattern

The list query applies filters only when provided, using PostgreSQL's
`IS NULL` short-circuit:

```sql
AND ($3::TEXT        IS NULL OR platform::TEXT = $3)
AND ($4::SMALLINT   IS NULL OR rating >= $4)
AND ($5::TIMESTAMPTZ IS NULL OR published_at >= $5)
AND ($6::TIMESTAMPTZ IS NULL OR published_at <= $6)
```

Passing `None` for a filter binds `NULL`, which the `IS NULL` check
bypasses. This allows a single parameterized query to handle all
combinations of filters without dynamic SQL string building.

### External Clients Gracefully Skip Missing Keys

Each client's `fetch_reviews` method returns `Ok(vec![])` immediately when
its API key is empty:

```rust
if self.api_key.is_empty() {
    tracing::debug!("Google Places API key not configured — skipping");
    return Ok(vec![]);
}
```

The service counts platforms whose restaurant ID is set but whose client
returned an empty list as "checked" without a warning. This allows mixed
configurations (e.g. Google configured, Yelp not) without noisy error logs.

### TripAdvisor Status

The `TripAdvisorClient` is fully implemented but not yet wired into the sync
loop. TripAdvisor's Content API requires an approved partnership application,
and the restaurant entity does not yet have a `tripadvisor_location_id` column.
The client will be activated when a future migration adds that column.

---

## New Configuration

```toml
# config/default.toml
[external_apis]
google_places_api_key = ""
yelp_api_key          = ""
tripadvisor_api_key   = ""
```

| Environment variable | Description |
|---|---|
| `APP__EXTERNAL_APIS__GOOGLE_PLACES_API_KEY` | Google Cloud Console → Places API |
| `APP__EXTERNAL_APIS__YELP_API_KEY` | Yelp Fusion portal → Manage App |
| `APP__EXTERNAL_APIS__TRIPADVISOR_API_KEY` | TripAdvisor Content API (partnership required) |

All three default to empty strings. Empty = platform skipped during sync.

---

## Database Schema

```sql
CREATE TYPE review_platform AS ENUM ('google', 'yelp', 'tripadvisor');

CREATE TABLE reviews (
    id               UUID            PRIMARY KEY DEFAULT uuid_generate_v4(),
    restaurant_id    UUID            NOT NULL REFERENCES restaurants(id) ON DELETE CASCADE,
    tenant_id        UUID            NOT NULL REFERENCES tenants(id)     ON DELETE CASCADE,
    platform         review_platform NOT NULL,
    external_id      TEXT            NOT NULL,
    author_name      TEXT            NOT NULL,
    rating           SMALLINT        NOT NULL CHECK (rating BETWEEN 1 AND 5),
    body             TEXT,
    published_at     TIMESTAMPTZ     NOT NULL,
    sentiment_score  REAL,           -- populated by Phase 4
    ai_reply_draft   TEXT,           -- populated by Phase 4
    created_at       TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ     NOT NULL DEFAULT NOW(),
    CONSTRAINT reviews_unique_external UNIQUE (restaurant_id, platform, external_id)
);
```

Indexes:
- `(tenant_id, restaurant_id, published_at DESC, id DESC)` — covers the list query
- `(restaurant_id, platform)` — covers platform-filtered lookups

---

## API Reference

### `POST /api/v1/restaurants/:id/reviews/sync`

Syncs reviews from all configured external platforms for the restaurant.
Platforms whose ID field is not set on the restaurant are skipped silently.

**Response `200 OK`:**
```json
{
  "reviews_synced": 5,
  "platforms_checked": ["google"],
  "warnings": []
}
```

`warnings` contains non-fatal platform errors (e.g. API returned an error for
one platform while another succeeded).

**Response `404`:** restaurant not found or belongs to another tenant.

---

### `GET /api/v1/restaurants/:id/reviews`

Returns a cursor-paginated, newest-first list of reviews.

**Query parameters:**

| Param | Type | Description |
|---|---|---|
| `limit` | integer | Items per page (1–100, default 20) |
| `cursor` | string | Opaque cursor from previous `next_cursor` |
| `platform` | string | Filter: `google`, `yelp`, or `tripadvisor` |
| `min_rating` | integer | Minimum star rating (1–5) |
| `from` | RFC 3339 | Earliest `published_at` to include |
| `to` | RFC 3339 | Latest `published_at` to include |

**Response `200 OK`:**
```json
{
  "items": [
    {
      "id": "uuid",
      "platform": "google",
      "external_id": "1704067200-jane_smith",
      "author_name": "Jane Smith",
      "rating": 5,
      "body": "Absolutely outstanding food and service.",
      "published_at": "2024-01-01T12:00:00Z",
      "sentiment_score": null,
      "ai_reply_draft": null,
      "created_at": "2024-01-02T08:00:00Z"
    }
  ],
  "next_cursor": "MTcwNDI1..."
}
```

`sentiment_score` and `ai_reply_draft` are always `null` in Phase 3 and
will be populated by Phase 4 (AI analysis).

**Response `422`:** unknown `platform` value.  
**Response `404`:** restaurant not found or belongs to another tenant.

---

## External API Notes

### Google Places (legacy Details endpoint)

```
GET https://maps.googleapis.com/maps/api/place/details/json
    ?place_id={restaurant.google_place_id}
    &fields=reviews
    &key={api_key}
```

Returns up to 5 reviews per place in the free tier. The `place_id` field on
the restaurant entity (e.g. `ChIJN1t_tDeuEmsRUsoyG83frY4`) is the value
required by this endpoint.

`external_id` is synthesised as `{unix_timestamp}-{author_slug}` since the
legacy API does not provide a stable review ID.

### Yelp Fusion

```
GET https://api.yelp.com/v3/businesses/{restaurant.yelp_id}/reviews?limit=50
Authorization: Bearer {api_key}
```

Returns up to 3 reviews in the free tier. `external_id` uses Yelp's stable
review UUID.

### TripAdvisor (not yet active)

Requires a dedicated `tripadvisor_location_id` column (planned for a future
migration) and a partnership-approved API key.

---

## New Dependencies

| Crate | Version | Used in | Purpose |
|---|---|---|---|
| `reqwest` | 0.12 | `forgebike-infrastructure` | HTTP client for external platform APIs |

---

## What Phase 4 Will Add

- `async-openai` adapter implementing a new `AiAnalysisPort`
- Sentiment scoring on review ingest (`sentiment_score` populated)
- `POST /restaurants/:id/reviews/:rid/reply-draft` — AI-generated response
- Prompt templates stored as versioned files in `crates/ai/prompts/`
- Per-tenant AI token usage tracking in Redis

See [`architecture.md`](./architecture.md) for the full multi-phase plan.
