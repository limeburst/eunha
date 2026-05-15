ALTER TABLE statuses ADD COLUMN idempotency_key text;
CREATE UNIQUE INDEX statuses_idempotency_key_idx ON statuses (account_id, idempotency_key) WHERE idempotency_key IS NOT NULL;
