CREATE TABLE IF NOT EXISTS canonical_email_blocks (
    id BIGSERIAL PRIMARY KEY,
    canonical_email_hash TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
