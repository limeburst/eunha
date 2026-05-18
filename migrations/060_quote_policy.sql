-- Migration 060: Quote policy system
ALTER TABLE statuses ADD COLUMN interaction_policy JSONB;

CREATE TABLE quotes (
    id BIGINT PRIMARY KEY,
    status_id BIGINT NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    quoted_status_id BIGINT NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    quoted_account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    activity_uri TEXT UNIQUE,
    approval_uri TEXT UNIQUE,
    state TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX quotes_status_id_idx ON quotes (status_id);
CREATE INDEX quotes_quoted_status_id_idx ON quotes (quoted_status_id);
CREATE INDEX quotes_account_id_idx ON quotes (account_id);
CREATE INDEX quotes_quoted_account_id_idx ON quotes (quoted_account_id);
CREATE INDEX quotes_state_idx ON quotes (state);
