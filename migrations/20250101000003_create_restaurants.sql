-- ---------------------------------------------------------------------------
-- Migration: 0003 — restaurants
--
-- Each tenant may own many restaurant locations.  The `tenant_id` column
-- is present on every row so that every query can be scoped to a single
-- tenant without a JOIN, enforcing the multi-tenant isolation guarantee
-- at the database level.
-- ---------------------------------------------------------------------------

CREATE TABLE restaurants (
    id              UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id       UUID        NOT NULL REFERENCES tenants(id)  ON DELETE CASCADE,
    name            TEXT        NOT NULL,
    description     TEXT,
    cuisine_type    TEXT,
    address         TEXT,
    phone           TEXT,
    website         TEXT,
    -- External platform IDs — populated by future review-sync phases.
    google_place_id TEXT,
    yelp_id         TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Tenant-scoped lookups (list, paginate).
CREATE INDEX idx_restaurants_tenant_id
    ON restaurants (tenant_id, created_at, id);

CREATE TRIGGER restaurants_set_updated_at
    BEFORE UPDATE ON restaurants
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
