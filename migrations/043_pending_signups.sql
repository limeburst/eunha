CREATE TABLE pending_signups (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id         UUID NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    username            TEXT NOT NULL,
    email               TEXT NOT NULL,
    email_normalized    TEXT NOT NULL,
    password_hash       TEXT NOT NULL,
    invite_id           UUID REFERENCES invites(id),
    reason              TEXT,
    locale              TEXT NOT NULL DEFAULT 'en',
    app_id              UUID REFERENCES oauth_applications(id),
    confirmation_token  TEXT NOT NULL UNIQUE,
    expires_at          TIMESTAMPTZ NOT NULL DEFAULT now() + interval '24 hours',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (instance_id, email_normalized)
);
