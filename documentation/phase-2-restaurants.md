# Phase 2 — Restaurants & Menu Management

> **Status**: Complete  
> **Timeframe**: Week 3  
> **Exit criterion**: Full CRUD for restaurants and menu items; cursor-based pagination on all list endpoints; tenant isolation enforced; 85/85 tests pass.

---

## What Was Built

| Deliverable | Location |
|---|---|
| Migrations: `restaurants`, `menu_items` tables | `migrations/0003`, `migrations/0004` |
| `Restaurant` + `MenuItem` domain entities | `crates/domain/src/entities/` |
| Cursor-based pagination primitives | `crates/domain/src/pagination.rs` |
| Port traits: `RestaurantRepository`, `MenuItemRepository` | `crates/domain/src/ports/` |
| `RestaurantService` — all CRUD use cases | `crates/application/src/restaurant/` |
| `PgRestaurantRepository`, `PgMenuItemRepository` | `crates/infrastructure/src/db/` |
| HTTP pagination helpers + cursor encode/decode | `crates/api/src/pagination.rs` |
| 9 REST handlers (5 restaurant + 4 menu item) | `crates/api/src/handlers/restaurants.rs` |
| `restaurant_routes` sub-router with auth middleware | `crates/api/src/router.rs` |
| `RestaurantError → ApiError` conversion | `crates/api/src/error.rs` |
| `restaurant_service` field added to `AppState` | `crates/api/src/state.rs` |
| Phase 2 tests (41 new assertions) | `scripts/test.sh` |

---

## Architecture Decisions

### Cursor-Based Pagination

All list endpoints use a cursor anchored on `(created_at, id)` rather than
`OFFSET n`.

**Why not OFFSET?**  Offset pagination returns inconsistent results when rows
are inserted between two page fetches — an item can appear on two consecutive
pages or be skipped entirely. Cursor-based pagination is immune to this because
the anchor is a stable position in the data, not a count of preceding rows.

**How the cursor works:**

```
first page:  cursor = (epoch, nil-UUID)   ← less than any real row
next pages:  cursor = (last_row.created_at, last_row.id)
```

The **n+1 trick** detects whether more pages exist without a separate `COUNT`
query: fetch `limit + 1` rows. If `limit + 1` rows are returned, truncate to
`limit` and make the last row the `next_cursor`. If fewer rows are returned,
set `next_cursor = null`.

**Cursor encoding** (API layer only):

```
raw format: "{epoch_milliseconds}:{uuid_hyphenated}"
wire format: URL-safe base64-no-padding of the raw string
```

The domain layer only knows about the typed `Cursor { created_at, id }` struct.
The `crates/api/src/pagination.rs` module owns the encode/decode logic using
the `base64 0.22` crate.

### Fetch-Then-Update PATCH Pattern

Rather than building a dynamic `UPDATE SET col = COALESCE($n, col)` SQL query
(which requires complex conditional binding), the service uses a three-step
fetch-then-update pattern:

1. **Fetch** the existing row (also verifies tenant ownership).
2. **Merge** the patch in Rust using struct spread: `Restaurant { name: cmd.name.unwrap_or(existing.name), ..existing }`.
3. **Persist** the full updated entity with a fixed-column `UPDATE … RETURNING`.

This is simpler, reads cleanly, and costs one extra SELECT — acceptable since
updates are infrequent relative to reads.

**Limitation in Phase 2:** Setting a nullable field back to `NULL` via PATCH is
not yet supported — omitting a field keeps the existing value; including a field
sets it to the new value. Full nullable-field clearing will be added in a later
phase using `Option<Option<T>>` deserialization.

### Tenant Isolation

Every table carries `tenant_id`. Every repository query scopes to `tenant_id`
in the WHERE clause:

```sql
WHERE tenant_id = $1 AND id = $2
```

The service layer extracts `tenant_id` from the `AuthIdentity` injected by the
auth middleware — it is never trusted from the request body or URL parameters.

Deleting a restaurant cascades to its menu items via the foreign key
`ON DELETE CASCADE` constraint defined in the migration.

### Menu Item Validation Against Parent Restaurant

Before creating or listing menu items, the service verifies the restaurant
exists **and belongs to the authenticated tenant**:

```rust
let _ = self.restaurants
    .find_by_id(identity.tenant_id, restaurant_id)
    .await?
    .ok_or(RestaurantError::RestaurantNotFound(restaurant_id))?;
```

This prevents one tenant from adding items to another tenant's restaurant by
guessing its UUID. The error surface (404) is indistinguishable from "the
restaurant simply doesn't exist."

### Price as Integer Cents

`price_cents: Option<i64>` stores prices in the smallest currency unit
(pence, cents, etc.) to avoid floating-point representation errors.
`None` means "price not set" (e.g. market price or price on request).

The API consumer is responsible for formatting the integer for display (e.g.
`2850` → `£28.50`).

---

## Database Schema

```sql
-- restaurants
CREATE TABLE restaurants (
    id              UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    name            TEXT        NOT NULL,
    description     TEXT,
    cuisine_type    TEXT,
    address         TEXT,
    phone           TEXT,
    website         TEXT,
    google_place_id TEXT,   -- populated by review-sync (Phase 3)
    yelp_id         TEXT,   -- populated by review-sync (Phase 3)
    created_at      TIMESTAMPTZ NOT NULL,
    updated_at      TIMESTAMPTZ NOT NULL
);

-- menu_items
CREATE TABLE menu_items (
    id            UUID        PRIMARY KEY,
    restaurant_id UUID        NOT NULL REFERENCES restaurants(id) ON DELETE CASCADE,
    tenant_id     UUID        NOT NULL REFERENCES tenants(id)     ON DELETE CASCADE,
    name          TEXT        NOT NULL,
    description   TEXT,
    price_cents   BIGINT,
    category      TEXT,
    is_available  BOOLEAN     NOT NULL DEFAULT true,
    created_at    TIMESTAMPTZ NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL
);
```

Both tables inherit the `set_updated_at()` trigger from Phase 0 and include a
composite index on `(tenant_id, created_at, id)` to serve cursor-paginated
queries efficiently.

---

## API Reference

All endpoints require `Authorization: Bearer <access_token>`.

### Restaurants

#### `POST /api/v1/restaurants`

Create a restaurant for the authenticated tenant.

```json
// Request
{
  "name": "The Golden Fork",          // required, 1–200 chars
  "description": "...",               // optional
  "cuisine_type": "Modern European",  // optional
  "address": "1 Harbour Lane",        // optional
  "phone": "+44 20 7946 0001",        // optional
  "website": "https://example.com"    // optional, must be valid URL
}

// Response 201
{
  "id": "uuid",
  "name": "The Golden Fork",
  "description": "...",
  "cuisine_type": "Modern European",
  "address": "1 Harbour Lane",
  "phone": "+44 20 7946 0001",
  "website": null,
  "google_place_id": null,
  "yelp_id": null,
  "created_at": "2025-01-01T00:00:00Z",
  "updated_at": "2025-01-01T00:00:00Z"
}
```

---

#### `GET /api/v1/restaurants`

List restaurants for the authenticated tenant with cursor pagination.

Query params: `?limit=20&cursor=<opaque_string>`

```json
// Response 200
{
  "items": [ { ...restaurant... }, ... ],
  "next_cursor": "MTczNjM4MDgwMDAwMDo..."   // null if no more pages
}
```

---

#### `GET /api/v1/restaurants/:id`

Get a single restaurant. Returns `404` if not found or belongs to another tenant.

---

#### `PATCH /api/v1/restaurants/:id`

Partial update. Fields omitted from the body are left unchanged.

```json
// Request — only include fields to change
{ "name": "The Silver Spoon", "cuisine_type": "Italian" }

// Response 200 — full updated restaurant object
```

---

#### `DELETE /api/v1/restaurants/:id`

Delete a restaurant and all its menu items (cascade). Returns `204 No Content`
on success, `404` if not found.

---

### Menu Items

#### `POST /api/v1/restaurants/:id/menu`

Add a menu item to a restaurant.

```json
// Request
{
  "name": "Beef Bourguignon",    // required
  "description": "...",          // optional
  "price_cents": 2850,           // optional, must be ≥ 0
  "category": "Mains",           // optional
  "is_available": true           // optional, defaults to true
}

// Response 201
{
  "id": "uuid",
  "restaurant_id": "uuid",
  "name": "Beef Bourguignon",
  "description": "...",
  "price_cents": 2850,
  "category": "Mains",
  "is_available": true,
  "created_at": "...",
  "updated_at": "..."
}
```

---

#### `GET /api/v1/restaurants/:id/menu`

List menu items for a restaurant. Same pagination shape as the restaurant list.

Query params: `?limit=20&cursor=<opaque_string>`

---

#### `PATCH /api/v1/restaurants/:id/menu/:item_id`

Partial update on a menu item. Returns `422` if the item belongs to a
different restaurant than the one in the URL.

---

#### `DELETE /api/v1/restaurants/:id/menu/:item_id`

Delete a menu item. Returns `204` on success, `404` if not found.

---

## Updated Crate Dependency Graph

```
forgebike-server  (composition root)
    ├── forgebike-api
    │       ├── forgebike-config
    │       ├── forgebike-domain
    │       └── forgebike-application
    │               ├── forgebike-config
    │               └── forgebike-domain    ← Cursor, Page, Restaurant, MenuItem
    ├── forgebike-infrastructure
    │       └── forgebike-domain
    └── forgebike-config
```

---

## New Dependencies

| Crate | Version | Used in | Purpose |
|---|---|---|---|
| `base64` | 0.22 | `forgebike-api` | Cursor encode/decode for HTTP wire format |

---

## What Phase 3 Will Add

Phase 3 wires in the external review platforms:

- `crates/infrastructure/src/review_clients/` — async `reqwest` clients for Google Places, Yelp Fusion, and TripAdvisor Content APIs
- `ReviewFetchPort` trait in domain
- `SyncReviewsJob` (apalis) — fetches, deduplicates by `external_id`, persists
- `POST /api/v1/restaurants/:id/reviews/sync` — enqueues the job, returns 202 Accepted
- `GET /api/v1/restaurants/:id/reviews` — paginated, filterable by platform / rating / date range
- Migration: `reviews` table

See [`architecture.md`](./architecture.md) for the full multi-phase plan.
