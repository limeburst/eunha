CREATE TABLE annual_reports (
    id            BIGSERIAL PRIMARY KEY,
    account_id    BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    year          INT NOT NULL,
    data          JSONB,
    schema_version INT NOT NULL DEFAULT 1,
    share_key     TEXT,
    viewed_at     TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (account_id, year)
);
