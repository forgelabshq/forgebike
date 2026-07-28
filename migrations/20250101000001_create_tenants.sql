-- ---------------------------------------------------------------------------
-- Migration: 0001 — tenants
--
-- The `tenants` table is the top-level isolation boundary for the SaaS
-- platform.  Every other table will carry a `tenant_id` foreign key.
-- ---------------------------------------------------------------------------

-- Enable the uuid extension (idempotent — safe to call even if it already
-- exists, which it will on most managed Postgres services).
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ---------------------------------------------------------------------------
-- Plan tier type
-- ---------------------------------------------------------------------------
CREATE TYPE plan_tier AS ENUM ('starter', 'growth', 'scale');

-- ---------------------------------------------------------------------------
-- tenants
-- ---------------------------------------------------------------------------
CREATE TABLE tenants (
    id                 UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    name               TEXT        NOT NULL,
    plan_tier          plan_tier   NOT NULL DEFAULT 'starter',
    stripe_customer_id TEXT        UNIQUE,          -- null until billing is set up
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Fast lookup by Stripe customer ID (used in webhook handling — Phase 8).
CREATE INDEX idx_tenants_stripe_customer_id
    ON tenants (stripe_customer_id)
    WHERE stripe_customer_id IS NOT NULL;

-- ---------------------------------------------------------------------------
-- Automatically keep updated_at current on every row update.
-- ---------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$;

CREATE TRIGGER tenants_set_updated_at
    BEFORE UPDATE ON tenants
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
