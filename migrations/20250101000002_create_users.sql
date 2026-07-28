-- ---------------------------------------------------------------------------
-- Migration: 0002 — users & tenant memberships
--
-- A `user` is an individual login.  Users belong to exactly one tenant and
-- have one of three roles within it.  The uniqueness constraint on
-- (tenant_id, email) means the same email address can register with
-- different tenants (common in agency/multi-brand setups) but only once
-- per tenant.
-- ---------------------------------------------------------------------------

CREATE TYPE user_role AS ENUM ('owner', 'manager', 'viewer');

-- ---------------------------------------------------------------------------
-- users
-- ---------------------------------------------------------------------------
CREATE TABLE users (
    id            UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id     UUID        NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    email         TEXT        NOT NULL,
    password_hash TEXT        NOT NULL,   -- argon2id hash, never plaintext
    role          user_role   NOT NULL DEFAULT 'viewer',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- One email address per tenant.
    CONSTRAINT users_tenant_email_unique UNIQUE (tenant_id, email)
);

CREATE INDEX idx_users_tenant_id ON users (tenant_id);
-- Lookup by email is needed at login time (before the tenant is known).
CREATE INDEX idx_users_email     ON users (email);

CREATE TRIGGER users_set_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- ---------------------------------------------------------------------------
-- refresh_tokens
--
-- Stored in Redis in Phase 1 (fast revocation, TTL-based expiry).
-- This table serves as a persistent audit log of issued refresh tokens and
-- is used for "log out all devices" scenarios.
-- ---------------------------------------------------------------------------
CREATE TABLE refresh_tokens (
    id         UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id    UUID        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT        NOT NULL UNIQUE, -- SHA-256 of the raw token
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,                 -- null = still valid
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_refresh_tokens_user_id ON refresh_tokens (user_id);
