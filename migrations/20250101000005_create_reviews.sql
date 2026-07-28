-- ---------------------------------------------------------------------------
-- Migration: 0005 — reviews
--
-- Reviews are aggregated from external platforms (Google, Yelp, TripAdvisor).
-- The combination (restaurant_id, platform, external_id) is globally unique,
-- which allows the sync process to use INSERT … ON CONFLICT to safely upsert
-- without duplicating reviews.
--
-- sentiment_score and ai_reply_draft are left NULL by the sync job and
-- populated by the AI analysis introduced in Phase 4.
-- ---------------------------------------------------------------------------

CREATE TYPE review_platform AS ENUM ('google', 'yelp', 'tripadvisor');

CREATE TABLE reviews (
    id               UUID             PRIMARY KEY DEFAULT uuid_generate_v4(),
    restaurant_id    UUID             NOT NULL REFERENCES restaurants(id) ON DELETE CASCADE,
    tenant_id        UUID             NOT NULL REFERENCES tenants(id)     ON DELETE CASCADE,
    platform         review_platform  NOT NULL,
    -- Platform-assigned identifier — used for deduplication.
    external_id      TEXT             NOT NULL,
    author_name      TEXT             NOT NULL,
    -- 1–5 star rating stored as a smallint.
    rating           SMALLINT         NOT NULL CHECK (rating BETWEEN 1 AND 5),
    body             TEXT,
    published_at     TIMESTAMPTZ      NOT NULL,
    -- Populated by Phase 4 AI sentiment analysis (-1.0 … 1.0).
    sentiment_score  REAL,
    -- Populated by Phase 4 AI reply generation.
    ai_reply_draft   TEXT,
    created_at       TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ      NOT NULL DEFAULT NOW(),

    -- Prevents the same external review from being inserted twice.
    CONSTRAINT reviews_unique_external
        UNIQUE (restaurant_id, platform, external_id)
);

-- Covers the common list query: tenant + restaurant, newest first.
CREATE INDEX idx_reviews_restaurant
    ON reviews (tenant_id, restaurant_id, published_at DESC, id DESC);

-- Speeds up platform-filtered list queries.
CREATE INDEX idx_reviews_platform
    ON reviews (restaurant_id, platform);

CREATE TRIGGER reviews_set_updated_at
    BEFORE UPDATE ON reviews
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
