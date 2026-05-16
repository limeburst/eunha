-- Index to efficiently fetch statuses by account_id ordered by id (home timeline).
-- The existing statuses_by_account_created_at index doesn't cover ORDER BY id,
-- so the home timeline query was doing a full table scan.
CREATE INDEX IF NOT EXISTS statuses_by_account_id_desc
    ON statuses(account_id, id DESC)
    WHERE deleted_at IS NULL;
