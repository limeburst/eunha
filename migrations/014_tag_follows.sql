CREATE TABLE tag_follows (
    id          BIGSERIAL PRIMARY KEY,
    account_id  UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    tag_id      UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, tag_id)
);

CREATE INDEX tag_follows_by_account ON tag_follows(account_id);
