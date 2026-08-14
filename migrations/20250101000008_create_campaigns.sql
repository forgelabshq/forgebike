-- ---------------------------------------------------------------------------
-- Migration: 0008 — campaigns
--
-- Bulk-message campaigns sent to a filtered set of customer contacts.
-- Lifecycle: draft → sending → sent (or failed).
--
-- tag_filter is nullable: NULL = all contacts for this restaurant.
-- ---------------------------------------------------------------------------

CREATE TYPE campaign_channel AS ENUM ('email', 'sms');
CREATE TYPE campaign_status  AS ENUM ('draft', 'scheduled', 'sending', 'sent', 'failed');

CREATE TABLE campaigns (
    id               UUID             PRIMARY KEY DEFAULT uuid_generate_v4(),
    tenant_id        UUID             NOT NULL REFERENCES tenants(id)      ON DELETE CASCADE,
    restaurant_id    UUID             NOT NULL REFERENCES restaurants(id)  ON DELETE CASCADE,
    name             TEXT             NOT NULL,
    channel          campaign_channel NOT NULL DEFAULT 'email',
    status           campaign_status  NOT NULL DEFAULT 'draft',
    -- Email subject line (required for email campaigns).
    subject          TEXT,
    -- Message body (plain text).
    body             TEXT             NOT NULL,
    -- Audience filter: only contacts with this tag receive the campaign.
    -- NULL = all contacts.
    tag_filter       TEXT,
    -- Informational future-send timestamp (auto-dispatching not yet implemented).
    scheduled_at     TIMESTAMPTZ,
    -- Set when the send background task completes.
    sent_at          TIMESTAMPTZ,
    -- Number of recipients the campaign was dispatched to.
    recipients_count INTEGER          NOT NULL DEFAULT 0,
    created_at       TIMESTAMPTZ      NOT NULL DEFAULT NOW(),
    updated_at       TIMESTAMPTZ      NOT NULL DEFAULT NOW()
);

-- Covers the common list query: all campaigns for a restaurant, newest-first.
CREATE INDEX idx_campaigns_restaurant
    ON campaigns (tenant_id, restaurant_id, created_at DESC, id DESC);

-- Covers status-filtered list queries.
CREATE INDEX idx_campaigns_status
    ON campaigns (restaurant_id, status);

CREATE TRIGGER campaigns_set_updated_at
    BEFORE UPDATE ON campaigns
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
