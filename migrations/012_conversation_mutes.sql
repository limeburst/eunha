CREATE TABLE conversation_mutes (
    id          BIGSERIAL PRIMARY KEY,
    account_id  UUID   NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    status_id   BIGINT NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, status_id)
);
