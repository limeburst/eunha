-- Speed up poll expiry background job (was doing a full table scan)
CREATE INDEX IF NOT EXISTS polls_by_expires_at
    ON polls(expires_at) WHERE expires_at IS NOT NULL;

-- Speed up MAU/WAU and activity queries that filter by account_id + created_at
CREATE INDEX IF NOT EXISTS statuses_by_account_created_at
    ON statuses(account_id, created_at DESC) WHERE deleted_at IS NULL;

-- Speed up pinned-statuses fetch (ORDER BY sp.id DESC per account)
CREATE INDEX IF NOT EXISTS status_pins_by_account
    ON status_pins(account_id, id DESC);
