-- Speed up the public timeline query (was doing a full table scan + sort)
-- Covers the static WHERE conditions and supports ORDER BY id DESC/ASC with LIMIT.
CREATE INDEX IF NOT EXISTS statuses_public_timeline
    ON statuses(instance_id, id DESC)
    WHERE visibility = 'public'
      AND deleted_at IS NULL
      AND reblog_of_id IS NULL
      AND (NOT reply OR in_reply_to_account_id = account_id);
