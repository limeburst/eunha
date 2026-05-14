CREATE TABLE IF NOT EXISTS account_aliases (
    id         BIGSERIAL PRIMARY KEY,
    account_id UUID        NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    uri        TEXT        NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, uri)
);
