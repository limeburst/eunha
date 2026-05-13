CREATE INDEX IF NOT EXISTS statuses_by_reply
    ON statuses (in_reply_to_id)
    WHERE in_reply_to_id IS NOT NULL AND deleted_at IS NULL;
