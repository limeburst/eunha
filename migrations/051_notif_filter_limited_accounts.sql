ALTER TABLE users
    ADD COLUMN IF NOT EXISTS notif_filter_limited_accounts BOOLEAN NOT NULL DEFAULT false;
