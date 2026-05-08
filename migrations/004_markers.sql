CREATE TABLE IF NOT EXISTS markers (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id  UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    timeline    TEXT NOT NULL CHECK (timeline IN ('home', 'notifications')),
    last_read_id TEXT NOT NULL DEFAULT '',
    version     INT NOT NULL DEFAULT 0,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, timeline)
);
