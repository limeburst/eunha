-- VAPID keypairs for each instance
ALTER TABLE instances
    ADD COLUMN vapid_private_key TEXT NOT NULL DEFAULT '',
    ADD COLUMN vapid_public_key  TEXT NOT NULL DEFAULT '';

-- One Web Push subscription per access token
CREATE TABLE web_push_subscriptions (
    id              BIGSERIAL PRIMARY KEY,
    account_id      UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    access_token_id UUID NOT NULL REFERENCES oauth_access_tokens(id) ON DELETE CASCADE,
    endpoint        TEXT NOT NULL,
    p256dh          TEXT NOT NULL,
    auth            TEXT NOT NULL,
    alert_follow    BOOLEAN NOT NULL DEFAULT true,
    alert_favourite BOOLEAN NOT NULL DEFAULT true,
    alert_reblog    BOOLEAN NOT NULL DEFAULT true,
    alert_mention   BOOLEAN NOT NULL DEFAULT true,
    alert_poll      BOOLEAN NOT NULL DEFAULT false,
    alert_status    BOOLEAN NOT NULL DEFAULT false,
    policy          TEXT NOT NULL DEFAULT 'all',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (access_token_id)
);
