-- ---------------------------------------------------------------------------
-- Migration: 0004 — menu_items
--
-- Menu items belong to a restaurant.  `tenant_id` is denormalized onto
-- this table so we can filter by tenant in a single index scan without
-- joining up through `restaurants`.
-- ---------------------------------------------------------------------------

CREATE TABLE menu_items (
    id            UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    restaurant_id UUID        NOT NULL REFERENCES restaurants(id) ON DELETE CASCADE,
    tenant_id     UUID        NOT NULL REFERENCES tenants(id)     ON DELETE CASCADE,
    name          TEXT        NOT NULL,
    description   TEXT,
    -- Price stored in the currency's smallest unit (pence, cents, etc.)
    -- to avoid floating-point rounding.  NULL = price not set / market price.
    price_cents   BIGINT,
    category      TEXT,
    is_available  BOOLEAN     NOT NULL DEFAULT true,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Most list queries will be restaurant-scoped within a tenant.
CREATE INDEX idx_menu_items_restaurant_id
    ON menu_items (tenant_id, restaurant_id, created_at, id);

CREATE TRIGGER menu_items_set_updated_at
    BEFORE UPDATE ON menu_items
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
