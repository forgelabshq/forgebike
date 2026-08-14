# Phase 7 — Customer Engagement

> **Status**: Complete  
> **Timeframe**: Weeks 11–12  
> **Exit criterion**: A campaign can be sent; the AI chat widget answers restaurant questions; tenant isolation enforced; 10 new unit tests pass.

---

## Overview

Phase 7 adds customer engagement features to the Forgebike platform: contact management for storing and segmenting restaurant customers, bulk email campaigns dispatched via a background `tokio::spawn` loop, and a real-time AI chat widget delivered over WebSocket so diners can ask questions about the restaurant directly from its website.

---

## Architecture

```
HTTP/WS
   │
   ▼
API layer (axum handlers)
   │  ├── contacts.rs  (CRUD + bulk import)
   │  ├── campaigns.rs (CRUD + send)
   │  └── chat.rs      (WebSocket, JWT from ?token=)
   │
   ▼
Application layer
   │  ├── ContactService  (verify restaurant → delegate to repo)
   │  ├── CampaignService (CRUD + tokio::spawn send loop)
   │  └── AiService::chat (new method — restaurant context + OpenAI)
   │
   ▼
Infrastructure layer
   │  ├── PgCustomerContactRepository  (sqlx, TEXT[] tags, GIN index)
   │  ├── PgCampaignRepository         (sqlx, PostgreSQL enum cast to TEXT)
   │  ├── LettreEmailClient            (lettre 0.11, graceful if unconfigured)
   │  └── OpenAiClient::chat           (new method — multi-turn conversation)
```

---

## Design Decisions

| Decision | Rationale |
|---|---|
| `tokio::spawn` instead of apalis | Achieves the exit criterion without introducing job-queue infrastructure; apalis can be added in Phase 9 when scheduling is needed |
| lettre 0.11 with `tokio1-native-tls` | Async SMTP with TLS; graceful no-op when `smtp_host` is empty |
| SMS stub (501) | Requires Twilio OAuth setup; deferred to Phase 9 |
| JWT via `?token=` for WebSocket | WebSocket handshakes cannot carry custom headers reliably in all browsers |
| `COALESCE`-based PATCH | Simple single-query update; clearing a field to NULL deferred to future phase |
| Tag filtering via `= ANY(tags)` with GIN index | Efficient array containment without a join table |
| Client-side chat history | Keeps the server stateless; max 20 turns sent per request to bound tokens |

---

## Endpoints

All REST endpoints require `Authorization: Bearer <access_token>`. The WebSocket endpoint authenticates via `?token=<jwt>` in the URL.

| Method | Path | Description |
|---|---|---|
| `POST` | `/api/v1/restaurants/:id/contacts` | Create a contact |
| `GET` | `/api/v1/restaurants/:id/contacts` | List contacts (paginated, `?tag=` filter) |
| `GET` | `/api/v1/restaurants/:id/contacts/:cid` | Get a contact |
| `PATCH` | `/api/v1/restaurants/:id/contacts/:cid` | Update a contact |
| `DELETE` | `/api/v1/restaurants/:id/contacts/:cid` | Delete a contact |
| `POST` | `/api/v1/restaurants/:id/contacts/import` | Bulk-import contacts from JSON array |
| `POST` | `/api/v1/restaurants/:id/campaigns` | Create a campaign |
| `GET` | `/api/v1/restaurants/:id/campaigns` | List campaigns (`?status=` filter) |
| `GET` | `/api/v1/restaurants/:id/campaigns/:cid` | Get a campaign |
| `PATCH` | `/api/v1/restaurants/:id/campaigns/:cid` | Update a draft campaign |
| `DELETE` | `/api/v1/restaurants/:id/campaigns/:cid` | Delete a draft campaign |
| `POST` | `/api/v1/restaurants/:id/campaigns/:cid/send` | Dispatch campaign |
| `WS` | `/api/v1/ws/chat/:restaurant_id?token=<jwt>` | AI chat widget |

---

## Contact Request / Response

**`POST /api/v1/restaurants/:id/contacts` — request body:**
```json
{
  "name":  "Alice Liddell",
  "email": "alice@example.com",
  "phone": "+15550001234",
  "tags":  ["vip", "loyalty-gold"]
}
```

All fields except `email` are optional. `email` must be unique per restaurant; duplicate submissions are rejected with `409 Conflict`.

**Response `201 Created`:**
```json
{
  "id":            "550e8400-e29b-41d4-a716-446655440000",
  "restaurant_id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
  "name":          "Alice Liddell",
  "email":         "alice@example.com",
  "phone":         "+15550001234",
  "tags":          ["vip", "loyalty-gold"],
  "created_at":    "2025-06-01T09:00:00Z",
  "updated_at":    "2025-06-01T09:00:00Z"
}
```

---

## Campaign Lifecycle

```
draft → sending → sent
              ↘ failed
```

Only campaigns in the `draft` state can be updated (`PATCH`) or deleted (`DELETE`). Attempting either operation on a `sending`, `sent`, or `failed` campaign returns `409 Conflict`. Once `POST …/send` is called the campaign moves to `sending` immediately; the background task updates it to `sent` or `failed` when all messages have been dispatched.

---

## Bulk Import

`POST /api/v1/restaurants/:id/contacts/import` accepts a JSON body:

```json
{
  "contacts": [
    { "name": "Bob Smith",  "email": "bob@example.com",  "tags": ["vip"] },
    { "name": "Carol Jones","email": "carol@example.com","tags": [] }
  ]
}
```

The repository inserts all rows with `ON CONFLICT (restaurant_id, email) DO NOTHING`, so duplicate emails for the same restaurant are silently skipped rather than causing the entire import to fail. The response reports:

```json
{ "imported": 2, "skipped": 0 }
```

---

## Campaign Send Flow

When `POST /api/v1/restaurants/:id/campaigns/:cid/send` is received:

1. **Validate** — verify the campaign exists and belongs to the tenant's restaurant.
2. **Check channel** — `email` proceeds; `sms` returns `501 Not Implemented` immediately (Twilio deferred).
3. **Check config** — if `smtp_host` is empty the handler returns `503 Service Unavailable` with a descriptive message.
4. **Set status → `sending`** — campaign row is updated in the database before spawning work.
5. **Spawn background task** — `tokio::spawn` iterates the tagged contact list, sends one email per contact via `LettreEmailClient`, then sets status to `sent` (or `failed` on error).
6. **Return `202 Accepted`** — the caller is told the campaign is dispatching; it should poll `GET …/campaigns/:cid` to track completion.

---

## WebSocket Chat Protocol

Connect to `WS /api/v1/ws/chat/:restaurant_id?token=<jwt>`.

The JWT is validated on connection upgrade; an invalid or expired token closes the socket with code `4001`.

**Client → server (text frame, JSON):**
```json
{
  "history": [
    { "role": "user",      "content": "What time do you close?" },
    { "role": "assistant", "content": "We close at 10 pm." }
  ],
  "message": "Do you take reservations?"
}
```

`history` carries the conversation so far (up to 20 turns; older turns are dropped client-side). The server is fully stateless — each frame is an independent OpenAI call with restaurant context prepended as the system prompt.

**Server → client (text frame, JSON):**
```json
{
  "reply": "Yes! You can book a table via our website or by calling us.",
  "restaurant_id": "f47ac10b-58cc-4372-a567-0e02b2c3d479"
}
```

On error (e.g. OpenAI unavailable):
```json
{ "error": "AI service temporarily unavailable" }
```

---

## Configuration

Email sending is configured in `config/default.toml`:

```toml
[email]
smtp_host     = ""   # empty = email disabled
smtp_port     = 587
smtp_username = ""
smtp_password = ""
from_address  = "noreply@forgebike.ai"
from_name     = "Forgebike"
```

Environment variable equivalents (for production / Docker):

```
APP__EMAIL__SMTP_HOST=smtp.sendgrid.net
APP__EMAIL__SMTP_PORT=587
APP__EMAIL__SMTP_USERNAME=apikey
APP__EMAIL__SMTP_PASSWORD=SG.xxxxx
APP__EMAIL__FROM_ADDRESS=hello@myrestaurant.com
APP__EMAIL__FROM_NAME=My Restaurant
```

When `smtp_host` is empty the `LettreEmailClient` returns a graceful error at runtime rather than panicking at startup, so development environments with no SMTP credentials still start cleanly.

---

## Database Schema

```sql
CREATE TABLE customer_contacts (
    id            UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    restaurant_id UUID        NOT NULL REFERENCES restaurants(id) ON DELETE CASCADE,
    tenant_id     UUID        NOT NULL REFERENCES tenants(id)     ON DELETE CASCADE,
    name          TEXT,
    email         TEXT        NOT NULL,
    phone         TEXT,
    tags          TEXT[]      NOT NULL DEFAULT '{}',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (restaurant_id, email)
);

-- GIN index for efficient tag-based filtering: WHERE 'vip' = ANY(tags)
CREATE INDEX idx_customer_contacts_tags ON customer_contacts USING GIN (tags);

CREATE TYPE campaign_channel AS ENUM ('email', 'sms');
CREATE TYPE campaign_status  AS ENUM ('draft', 'sending', 'sent', 'failed');

CREATE TABLE campaigns (
    id            UUID             PRIMARY KEY DEFAULT uuid_generate_v4(),
    restaurant_id UUID             NOT NULL REFERENCES restaurants(id) ON DELETE CASCADE,
    tenant_id     UUID             NOT NULL REFERENCES tenants(id)     ON DELETE CASCADE,
    name          TEXT             NOT NULL,
    subject       TEXT,
    body          TEXT             NOT NULL,
    channel       campaign_channel NOT NULL DEFAULT 'email',
    status        campaign_status  NOT NULL DEFAULT 'draft',
    tag_filter    TEXT,            -- NULL = send to all contacts
    sent_count    INTEGER          NOT NULL DEFAULT 0,
    created_at    TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ      NOT NULL DEFAULT NOW()
);
```

---

## Unit Tests

### ContactService (`crates/application/src/contacts/service.rs`)

| Test | Covers |
|---|---|
| `create_ok` | Happy-path contact creation with all fields |
| `get_not_found` | Returns `ContactNotFound` for unknown ID |
| `wrong_tenant_denied` | Returns `RestaurantNotFound` when restaurant belongs to a different tenant |
| `list_scoped` | Listed contacts are filtered by restaurant and optional tag |
| `bulk_import_count` | Reports correct `imported` + `skipped` counts |

Run with:
```bash
cargo test -p forgebike-application contacts
```

### CampaignService (`crates/application/src/campaigns/service.rs`)

| Test | Covers |
|---|---|
| `create_ok` | Happy-path campaign creation, status defaults to `draft` |
| `send_email_not_configured` | Returns `EmailNotConfigured` when `smtp_host` is empty |
| `send_sms_not_available` | Returns `SmsNotAvailable` for `channel = sms` |
| `update_non_draft_rejected` | Returns `CampaignNotEditable` for a `sending` campaign |
| `delete_non_draft_rejected` | Returns `CampaignNotEditable` for a `sent` campaign |

Run with:
```bash
cargo test -p forgebike-application campaigns
```

---

## Files Added / Modified

| File | Change |
|---|---|
| `migrations/20250101000007_create_customer_contacts.sql` | **New** — `customer_contacts` table + GIN index |
| `migrations/20250101000008_create_campaigns.sql` | **New** — `campaigns` table + enums |
| `crates/domain/src/entities/customer_contact.rs` | **New** — `CustomerContact` entity |
| `crates/domain/src/entities/campaign.rs` | **New** — `Campaign` entity, `CampaignChannel`, `CampaignStatus` enums |
| `crates/domain/src/entities/mod.rs` | Added `pub mod customer_contact`, `pub mod campaign` |
| `crates/domain/src/ports/contact_repository.rs` | **New** — `ContactRepository` port trait |
| `crates/domain/src/ports/campaign_repository.rs` | **New** — `CampaignRepository` port trait |
| `crates/domain/src/ports/email_port.rs` | **New** — `EmailPort` trait |
| `crates/domain/src/ports/mod.rs` | Added new port modules |
| `crates/domain/src/ports/ai_port.rs` | Added `chat` method to `AiContentPort` |
| `crates/infrastructure/src/db/contact_repository.rs` | **New** — `PgCustomerContactRepository` |
| `crates/infrastructure/src/db/campaign_repository.rs` | **New** — `PgCampaignRepository` |
| `crates/infrastructure/src/db/mod.rs` | Re-exported new repositories |
| `crates/infrastructure/src/email/lettre_client.rs` | **New** — `LettreEmailClient` |
| `crates/infrastructure/src/email/mod.rs` | **New** — module declaration |
| `crates/infrastructure/src/ai/openai.rs` | Added `chat` method |
| `crates/infrastructure/src/lib.rs` | Added `pub mod email` |
| `crates/application/src/contacts/service.rs` | **New** — `ContactService` + unit tests |
| `crates/application/src/contacts/error.rs` | **New** — `ContactError` |
| `crates/application/src/contacts/mod.rs` | **New** — module declaration |
| `crates/application/src/campaigns/service.rs` | **New** — `CampaignService` + unit tests |
| `crates/application/src/campaigns/error.rs` | **New** — `CampaignError` |
| `crates/application/src/campaigns/mod.rs` | **New** — module declaration |
| `crates/application/src/lib.rs` | Added `pub mod contacts`, `pub mod campaigns` |
| `crates/api/src/handlers/contacts.rs` | **New** — 6 REST handlers |
| `crates/api/src/handlers/campaigns.rs` | **New** — 6 REST handlers |
| `crates/api/src/handlers/chat.rs` | **New** — WebSocket upgrade handler |
| `crates/api/src/handlers/mod.rs` | Added `pub mod contacts`, `pub mod campaigns`, `pub mod chat` |
| `crates/api/src/error.rs` | Added `From<ContactError>`, `From<CampaignError>` for `ApiError` |
| `crates/api/src/state.rs` | Added `contact_service`, `campaign_service` fields |
| `crates/api/src/router.rs` | Wired contact, campaign, and WebSocket routes |
| `crates/config/src/lib.rs` | Added `EmailConfig` struct |
| `crates/server/src/main.rs` | Instantiated new repos, services, email client; added to `AppState` |
| `scripts/test.sh` | Added `test_phase_7` function |
| `documentation/phase-7-engagement.md` | **New** — this document |
