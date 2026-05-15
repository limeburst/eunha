ALTER TABLE accounts ADD COLUMN IF NOT EXISTS last_status_at TIMESTAMPTZ;

-- Backfill from existing statuses
UPDATE accounts a
SET last_status_at = (
    SELECT MAX(s.created_at)
    FROM statuses s
    WHERE s.account_id = a.id
      AND s.deleted_at IS NULL
      AND s.reblog_of_id IS NULL
)
WHERE a.domain IS NULL;
