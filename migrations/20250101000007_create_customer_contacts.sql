-- ---------------------------------------------------------------------------
-- Migration: 0007 — customer_contacts
--
-- Marketing contacts collected by restaurant owners for campaign delivery.
-- Contacts are scoped to (tenant_id, restaurant_id) for multi-tenant isolation.
--
-- The tags column uses a PostgreSQL TEXT array so owners can apply multiple
-- audience labels (e.g. 'vip', 'newsletter', 'birthday') without a join table.
--
-- The unique index on (tenant_id, restaurant_id, email) prevents accidental
-- duplicate imports while still allowing contacts without an email address.
-- ---------------------------------------------------------------------------

CREATE TABLE customer_contacts (
    id            UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id     UUID        NOT NULL REFERENCES tenants(id)      ON DELETE CASCADE,
    restaurant_id UUID        NOT NULL REFERENCES restaurants(id)  ON DELETE CASCADE,
    name          TEXT        NOT NULL,
    email         TEXT,
    phone         TEXT,
    -- Free-form audience tags for segmentation (e.g. 'vip', 'newsletter').
    tags          TEXT[]      NOT NULL DEFAULT '{}',
    notes         TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Covers the common list query: all contacts for a restaurant, newest-first.
CREATE INDEX idx_contacts_restaurant
    ON customer_contacts (tenant_id, restaurant_id, created_at DESC, id DESC);

-- Tag containment index — used for audience-filtered campaign queries.
CREATE INDEX idx_contacts_tags
    ON customer_contacts USING GIN (tags);

-- Prevents duplicate email imports within the same restaurant.
CREATE UNIQUE INDEX idx_contacts_email_unique
    ON customer_contacts (tenant_id, restaurant_id, email)
    WHERE email IS NOT NULL;

CREATE TRIGGER customer_contacts_set_updated_at
    BEFORE UPDATE ON customer_contacts
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
