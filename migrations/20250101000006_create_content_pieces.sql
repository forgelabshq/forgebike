-- ---------------------------------------------------------------------------
-- Migration: 0006 — content_pieces
--
-- AI-generated marketing content for restaurants.  Each piece is created as
-- a `draft`, reviewed by the owner, and can be `approved` then `published`.
-- ---------------------------------------------------------------------------

CREATE TYPE content_type AS ENUM (
    'social_post',       -- Twitter / Instagram / Facebook post
    'email',             -- Email subject + body
    'menu_description',  -- Short appetizing description for a menu item
    'blog_intro'         -- Opening paragraphs for a blog article
);

CREATE TYPE content_status AS ENUM ('draft', 'approved', 'published');

CREATE TABLE content_pieces (
    id            UUID           PRIMARY KEY DEFAULT uuid_generate_v4(),
    restaurant_id UUID           NOT NULL REFERENCES restaurants(id) ON DELETE CASCADE,
    tenant_id     UUID           NOT NULL REFERENCES tenants(id)     ON DELETE CASCADE,
    content_type  content_type   NOT NULL,
    -- Optional AI-generated title (email subject line, blog headline, etc.)
    title         TEXT,
    body          TEXT           NOT NULL,
    status        content_status NOT NULL DEFAULT 'draft',
    created_at    TIMESTAMPTZ    NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ    NOT NULL DEFAULT NOW()
);

-- Covers tenant-scoped list queries ordered newest-first.
CREATE INDEX idx_content_pieces_restaurant
    ON content_pieces (tenant_id, restaurant_id, created_at DESC, id DESC);

-- Covers status- and type-filtered list queries.
CREATE INDEX idx_content_pieces_status
    ON content_pieces (restaurant_id, status, content_type);

CREATE TRIGGER content_pieces_set_updated_at
    BEFORE UPDATE ON content_pieces
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
