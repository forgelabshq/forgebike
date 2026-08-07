# Phase 5 — AI Marketing Content Generation

> **Status**: Complete  
> **Timeframe**: Weeks 7–8  
> **Exit criterion**: A restaurant can generate, list, stream, edit, approve, and delete AI marketing content; SSE streaming delivers tokens live; tenant isolation enforced; 131/131 tests pass.

---

## What Was Built

| Deliverable | Location |
|---|---|
| Migration: `content_pieces` table | `migrations/20250101000006_create_content_pieces.sql` |
| `ContentPiece` entity + `ContentType` + `ContentStatus` enums | `crates/domain/src/entities/content_piece.rs` |
| `ContentRepository` port trait | `crates/domain/src/ports/content_repository.rs` |
| Extended `AiContentPort` — `generate_content` + `stream_content` | `crates/domain/src/ports/ai_port.rs` |
| Prompt template (embedded at compile time) | `crates/infrastructure/src/ai/prompts/content.txt` |
| `OpenAiClient` extended with sync + streaming content generation | `crates/infrastructure/src/ai/openai.rs` |
| `PgContentRepository` — full CRUD + filtered paginated list | `crates/infrastructure/src/db/content_repository.rs` |
| `ContentService` — generate, stream, list, get, update, delete | `crates/application/src/content/service.rs` |
| `AiConfig.max_content_tokens` added | `crates/config/src/lib.rs` |
| 6 REST handlers + SSE streaming endpoint | `crates/api/src/handlers/content.rs` |
| Content routes mounted under `/:id/content/` | `crates/api/src/router.rs` |
| `ContentError → ApiError` conversion | `crates/api/src/error.rs` |
| Phase 5 tests (16 new curl assertions) | `scripts/test.sh` |
| 6 new unit tests in `content::service::tests` | `crates/application/src/content/service.rs` |

---

## Architecture Decisions

### Four Content Types

| Type | Value | Output |
|---|---|---|
| Social post | `social_post` | ≤240 chars + 2–3 hashtags |
| Email campaign | `email` | Subject line + 3-paragraph body |
| Menu description | `menu_description` | 2-sentence appetising description |
| Blog intro | `blog_intro` | 2-paragraph hook + preview |

For `email`, the model is instructed to put the subject on the first line. The infrastructure `split_title_body` helper extracts it, populating `ContentPiece.title`.

### Synchronous Generation + SSE Streaming — Two Separate Endpoints

The architecture document mentioned returning a 202 with a job ID. For consistency with Phases 3–4 (synchronous approach until apalis arrives in Phase 7), two endpoints are provided:

- **`POST /generate`** — generates synchronously, returns `201 Created` with the full piece. Best for background scripts or cases where the caller can wait 3–10 s.
- **`GET /stream`** — opens an SSE connection. Tokens arrive in real time; the final event signals completion with the saved piece ID. Best for interactive UIs where the user watches the text appear.

Both save the piece to the database with status `draft`.

### Streaming via Callback Pattern

The `AiContentPort` trait's `stream_content` method accepts an `Arc<dyn Fn(String) + Send + Sync>` callback rather than a tokio channel sender. This keeps `tokio` out of the domain layer while still enabling streaming:

```rust
// Domain port — no tokio dependency
async fn stream_content(
    &self,
    context:  &ContentContext,
    on_chunk: Arc<dyn Fn(String) + Send + Sync>,
) -> Result<ContentDraft, DomainError>;
```

The API handler creates a `tokio::sync::mpsc::unbounded_channel`, wraps the sender in a closure, and passes it as the callback:

```rust
let on_chunk: Arc<dyn Fn(String) + Send + Sync> = Arc::new(move |chunk| {
    let _ = tx.send(chunk);   // UnboundedSender::send is non-async — safe in a Fn
});
```

The `UnboundedReceiverStream` from `tokio-stream` converts the receiver into a `futures::Stream` for the SSE response.

### Single Prompt Template

All four content types share one template (`content.txt`). The per-type instructions are injected as `{{CONTENT_TYPE_INSTRUCTION}}` at the call site — no separate files needed. This keeps the template set small while still allowing per-type behaviour.

### Content Status Lifecycle

```
draft  →  approved  →  published
```

Status transitions are driven by `PATCH` requests. The platform does not enforce the sequence — a piece can be set to any status directly. Enforcement can be added in Phase 8 if required.

### Descending Cursor Pagination

Like reviews, content pieces list newest-first (`ORDER BY created_at DESC, id DESC`). The same `Cursor::desc_start()` sentinel is used for the first page.

---

## Database Schema

```sql
CREATE TYPE content_type   AS ENUM ('social_post', 'email', 'menu_description', 'blog_intro');
CREATE TYPE content_status AS ENUM ('draft', 'approved', 'published');

CREATE TABLE content_pieces (
    id            UUID           PRIMARY KEY DEFAULT uuid_generate_v4(),
    restaurant_id UUID           NOT NULL REFERENCES restaurants(id) ON DELETE CASCADE,
    tenant_id     UUID           NOT NULL REFERENCES tenants(id)     ON DELETE CASCADE,
    content_type  content_type   NOT NULL,
    title         TEXT,
    body          TEXT           NOT NULL,
    status        content_status NOT NULL DEFAULT 'draft',
    created_at    TIMESTAMPTZ    NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ    NOT NULL DEFAULT NOW()
);
```

---

## API Reference

All endpoints require `Authorization: Bearer <access_token>`.

### `POST /api/v1/restaurants/:id/content/generate`

Generate a piece of marketing content and store it as a `draft`.

**Request body:**
```json
{
  "content_type": "social_post",   // required: social_post | email | menu_description | blog_intro
  "topic": "summer menu launch",   // optional: focus or subject
  "tone": "warm and playful"       // optional: style hints
}
```

**Response `201 Created`:**
```json
{
  "id": "uuid",
  "content_type": "social_post",
  "title": null,
  "body": "🌞 Summer is here at Content Bistro! ...",
  "status": "draft",
  "created_at": "2025-01-01T12:00:00Z",
  "updated_at": "2025-01-01T12:00:00Z"
}
```

For `email`, `title` contains the subject line.

**Response `503`:** OpenAI API key not configured.  
**Response `422`:** Unknown `content_type` value.

---

### `GET /api/v1/restaurants/:id/content/stream`

Opens a Server-Sent Events stream. Content tokens are sent as `data:` events. The final event is `data: __done__:<content_id>` (success) or `data: __error__:<message>` (failure).

**Query params** — same shape as `POST /generate` body, passed as URL params.

**Example SSE stream:**
```
data: 🌞 Summer is here

data:  at Content Bistro!

data: __done__:550e8400-e29b-41d4-a716-446655440000
```

The client should `GET /restaurants/:id/content/:cid` with the returned ID to retrieve the complete saved piece.

---

### `GET /api/v1/restaurants/:id/content`

List content pieces, newest first, with optional filters.

**Query parameters:**

| Param | Type | Description |
|---|---|---|
| `limit` | integer | Items per page (1–100, default 20) |
| `cursor` | string | Opaque cursor from previous `next_cursor` |
| `status` | string | `draft` · `approved` · `published` |
| `content_type` | string | `social_post` · `email` · `menu_description` · `blog_intro` |

---

### `GET /api/v1/restaurants/:id/content/:cid`

Return a single content piece.

---

### `PATCH /api/v1/restaurants/:id/content/:cid`

Partial update. Omitted fields are left unchanged.

```json
{
  "title":  "New subject line",
  "body":   "Edited body text",
  "status": "approved"
}
```

---

### `DELETE /api/v1/restaurants/:id/content/:cid`

Delete a content piece. Returns `204 No Content`.

---

## New Configuration

| Variable | Default | Description |
|---|---|---|
| `APP__AI__MAX_CONTENT_TOKENS` | `600` | Token budget per content generation call |

---

## New Dependencies

| Crate | Version | Used in | Purpose |
|---|---|---|---|
| `futures` | 0.3 | `forgebike-infrastructure`, `forgebike-api` | `StreamExt` for iterating OpenAI's streaming response; `Stream` trait for SSE |
| `tokio-stream` | 0.1 | `forgebike-api` | `UnboundedReceiverStream` converts mpsc receiver to SSE stream |

---

## What Phase 6 Will Add

- `analytics_snapshots` table + nightly rollup job
- `GET /restaurants/:id/analytics/overview` — 30/90/365-day KPI summary
- `GET /restaurants/:id/analytics/reviews` — rating trends + platform breakdown
- `GET /restaurants/:id/analytics/content` — published vs draft ratio, top content types
- Competitor snapshot (public rating comparison for other Google Place IDs)
- Redis caching (5-min TTL) on all analytics endpoints

See [`architecture.md`](./architecture.md) for the full multi-phase plan.
