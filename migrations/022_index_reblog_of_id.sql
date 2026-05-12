CREATE INDEX IF NOT EXISTS statuses_by_reblog
    ON statuses(account_id, reblog_of_id)
    WHERE reblog_of_id IS NOT NULL AND deleted_at IS NULL;
