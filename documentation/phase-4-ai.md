# Phase 4 — AI Sentiment & Reply Drafts

> **Status**: Complete  
> **Timeframe**: Week 6  
> **Exit criterion**: Sentiment analysis runs on reviews via a dedicated endpoint; AI reply drafts are generated and persisted; token usage is tracked in Redis; `reply-publish` stub returns 501; 115/115 tests pass.

---

## What Was Built

| Deliverable | Location |
|---|---|
| `AiContentPort` domain port (sentiment + reply draft) | `crates/domain/src/ports/ai_port.rs` |
| `TokenUsageStore` domain port (monthly token counters) | `crates/domain/src/ports/token_usage_store.rs` |
| Extended `ReviewRepository` (4 new methods) | `crates/domain/src/ports/review_repository.rs` |
| `OpenAiClient` — `async-openai` adapter | `crates/infrastructure/src/ai/openai.rs` |
| Prompt templates (compile-time embedded) | `crates/infrastructure/src/ai/prompts/` |
| `RedisTokenUsageStore` — monthly counters in Redis | `crates/infrastructure/src/redis/token_usage.rs` |
| `AiService` — all AI use cases | `crates/application/src/ai/service.rs` |
| `AiConfig` added to `Config` | `crates/config/src/lib.rs` |
| `[ai]` section in `config/default.toml` | `config/default.toml` |
| 4 new REST handlers | `crates/api/src/handlers/ai.rs` |
| `/api/v1/ai/usage` route | `crates/api/src/router.rs` |
| `AiError → ApiError` conversion | `crates/api/src/error.rs` |
| Phase 4 tests (14 new assertions) | `scripts/test.sh` |
| 9 new unit tests in `ai::service::tests` | `crates/application/src/ai/service.rs` |

---

## Architecture Decisions

### `AiContentPort` — Two Operations, One Port

Rather than two separate ports (`SentimentPort`, `ReplyPort`), both AI
operations share a single `AiContentPort` trait. Both are implemented by the
same `OpenAiClient` struct and share the same API key and model configuration.
This avoids constructor proliferation without losing flexibility — a mock
implementation for tests can stub both methods independently.

```rust
pub trait AiContentPort: Send + Sync {
    async fn analyse_sentiment(&self, text: &str)
        -> Result<Option<SentimentResult>, DomainError>;

    async fn generate_reply_draft(&self, context: &ReplyContext)
        -> Result<ReplyDraft, DomainError>;
}
```

`analyse_sentiment` returns `Ok(None)` when the API key is empty — callers
treat this as a graceful skip. `generate_reply_draft` returns `Err` in the same
situation so callers can surface a `503 Service Unavailable` to the user.

### Sentiment via Dedicated Endpoint (not during sync)

The architecture document says "sentiment scoring on review ingest". In
practice, running an OpenAI call per review inside `sync_reviews` would make
a sync of 50 reviews take 50–150 seconds. Phase 4 implements a dedicated
`POST /reviews/analyse` endpoint instead:

- `sync_reviews` stays fast (upsert only, no AI calls)
- `analyse_pending_reviews` runs sentiment on up to 50 reviews per call
- The frontend calls both endpoints: sync first, then analyse
- If the AI key is unconfigured, `analyse` returns `analysed: 0` without error

This is more practical and aligns with how production AI pipelines work (async
batch processing rather than blocking ingest paths).

### Prompt Templates as Embedded Files

Prompts are stored as plain-text files in
`crates/infrastructure/src/ai/prompts/` and embedded at compile time via
`include_str!`:

```rust
const SENTIMENT_PROMPT: &str = include_str!("prompts/sentiment.txt");
const REPLY_PROMPT:     &str = include_str!("prompts/reply.txt");
```

**Why files, not string literals?**
- Non-engineers can read, review, and iterate on prompts without touching Rust
- Version control shows prompt diffs clearly
- No recompilation needed when only the prompt changes (though `include_str!`
  does recompile — future improvement: load at startup from configurable path)

Substitution uses simple `str::replace` for `{{PLACEHOLDER}}` tokens, keeping
the template system dependency-free.

### Token Usage Tracking

Redis key format: `ai:tokens:{tenant_id}:{YYYYMM}`  
Operations: `INCRBY` to record, `GET` to query  
TTL: 62 days (refreshed on every write)

Usage is **recorded but not enforced** in Phase 4. Enforcement (blocking calls
when a plan limit is reached) is Phase 8 (billing). The infrastructure and
port are already in place so Phase 8 only needs to add the enforcement check.

### Reply Publish — 501 Stub

`POST /restaurants/:id/reviews/:rid/reply-publish` returns `501 Not Implemented`.

Publishing replies to Google and Yelp requires:
- Google: a verified Google My Business account with an OAuth 2.0 flow
  (`https://www.googleapis.com/auth/business.manage`)
- Yelp: a Yelp Business Owner account — Yelp does not currently offer a
  programmatic reply API in their public Fusion tier

The endpoint is defined with the correct signature so the Python frontend can
integrate against it now, and the implementation will slot in without changing
the API surface.

### `ReviewRepository` Extended

Four new methods added to the existing port:

| Method | Purpose |
|---|---|
| `find_by_id(tenant_id, id)` | Look up a single review (used by reply-draft and get-review handlers) |
| `list_pending_analysis(tenant_id, restaurant_id, limit)` | Reviews with `sentiment_score IS NULL` and `body IS NOT NULL` |
| `update_sentiment(id, score)` | Write the AI-computed score |
| `save_reply_draft(id, draft)` | Persist the generated reply text |

The SQL for `list_pending_analysis` filters by `body IS NOT NULL` so bodyless
reviews never reach the AI service — the service also trims and skips
whitespace-only bodies as a second line of defence (this combination is what
the unit test `analyse_pending_skips_reviews_with_empty_body` tests).

---

## New Configuration

```toml
# config/default.toml
[ai]
openai_api_key       = ""          # APP__AI__OPENAI_API_KEY in production
model                = "gpt-4o-mini"
max_sentiment_tokens = 60
max_reply_tokens     = 300
```

| Variable | Default | Description |
|---|---|---|
| `APP__AI__OPENAI_API_KEY` | `""` | `platform.openai.com/api-keys` |
| `APP__AI__MODEL` | `gpt-4o-mini` | Any OpenAI chat completion model |
| `APP__AI__MAX_SENTIMENT_TOKENS` | `60` | Token budget for sentiment calls |
| `APP__AI__MAX_REPLY_TOKENS` | `300` | Token budget for reply draft calls |

When `openai_api_key` is empty:
- `POST /analyse` → `200` with `analysed: 0`
- `POST /reply-draft` → `503 Service Unavailable`

---

## API Reference

All endpoints require `Authorization: Bearer <access_token>`.

### `GET /api/v1/restaurants/:id/reviews/:rid`

Return a single review including its AI fields.

**Response `200 OK`:**
```json
{
  "id": "uuid",
  "platform": "google",
  "author_name": "Jane Smith",
  "rating": 5,
  "body": "Amazing food and service!",
  "published_at": "2024-01-01T12:00:00Z",
  "sentiment_score": 0.92,
  "ai_reply_draft": "Thank you so much, Jane! ...",
  "created_at": "2024-01-02T08:00:00Z"
}
```

`sentiment_score` is `null` until `POST /analyse` is called.  
`ai_reply_draft` is `null` until `POST /reply-draft` is called.

---

### `POST /api/v1/restaurants/:id/reviews/analyse`

Run AI sentiment analysis on all reviews that do not yet have a score
(up to 50 per call).

**Response `200 OK`:**
```json
{
  "analysed":    5,
  "skipped":     0,
  "tokens_used": 210
}
```

`analysed: 0` with no error when `APP__AI__OPENAI_API_KEY` is not configured.

---

### `POST /api/v1/restaurants/:id/reviews/:rid/reply-draft`

Generate an AI reply draft for a review and save it to the review record.

**Response `200 OK`:**
```json
{
  "review_id": "uuid",
  "draft": "Thank you for your wonderful 5-star review! We're delighted that..."
}
```

**Response `503 Service Unavailable`:** OpenAI API key not set.  
**Response `422 Unprocessable Entity`:** Review has no body text.

---

### `POST /api/v1/restaurants/:id/reviews/:rid/reply-publish`

**`501 Not Implemented`** — see [Architecture Decisions](#reply-publish--501-stub) above.

---

### `GET /api/v1/ai/usage`

Return the total OpenAI tokens used by the authenticated tenant in the
current calendar month.

**Response `200 OK`:**
```json
{ "monthly_tokens_used": 1450 }
```

---

## New Dependencies

| Crate | Version | Used in | Purpose |
|---|---|---|---|
| `async-openai` | 0.28 | `forgebike-infrastructure` | Typed async wrapper for the OpenAI API |

---

## What Phase 5 Will Add

- `GenerateContentJob` — AI-generated social posts, email copy, menu descriptions, blog intros
- `POST /restaurants/:id/content/generate` — queues a generation job
- `GET  /restaurants/:id/content` — paginated list of generated pieces
- `PATCH /restaurants/:id/content/:cid` — edit/approve a draft
- SSE streaming endpoint for live generation preview
- Per-tenant AI usage enforcement against plan limits

See [`architecture.md`](./architecture.md) for the full multi-phase plan.
