ALTER TABLE users
    ADD COLUMN IF NOT EXISTS notif_filter_not_following      BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS notif_filter_not_followers      BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS notif_filter_new_accounts       BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS notif_filter_private_mentions   BOOLEAN NOT NULL DEFAULT false;

CREATE TABLE IF NOT EXISTS notification_requests (
    id          BIGSERIAL PRIMARY KEY,
    account_id  UUID        NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    from_account_id UUID   NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    last_status_id BIGINT,
    notifications_count BIGINT NOT NULL DEFAULT 1,
    dismissed   BOOLEAN     NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, from_account_id)
);
