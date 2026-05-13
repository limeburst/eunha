-- Per-user posting defaults and privacy preferences.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS default_privacy   TEXT    NOT NULL DEFAULT 'public',
    ADD COLUMN IF NOT EXISTS default_sensitive BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS default_language  TEXT;

-- Per-account collection-visibility toggle (hides followers/following lists).
ALTER TABLE accounts
    ADD COLUMN IF NOT EXISTS hide_collections BOOLEAN NOT NULL DEFAULT false;
