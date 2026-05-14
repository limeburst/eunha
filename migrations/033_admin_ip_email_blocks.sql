CREATE TABLE IF NOT EXISTS admin_ip_blocks (
    id         BIGSERIAL PRIMARY KEY,
    ip         TEXT        NOT NULL UNIQUE,
    severity   TEXT        NOT NULL DEFAULT 'sign_up_block',
    comment    TEXT,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS admin_email_domain_blocks (
    id         BIGSERIAL PRIMARY KEY,
    domain     TEXT        NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
