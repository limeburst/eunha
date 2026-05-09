-- Console users (those who manage instances via the console UI)
CREATE TABLE console_users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email           TEXT NOT NULL,
    email_normalized TEXT NOT NULL UNIQUE,
    password_hash   TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Console session tokens (Bearer tokens stored in the browser)
CREATE TABLE console_sessions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    console_user_id UUID NOT NULL REFERENCES console_users(id) ON DELETE CASCADE,
    token           TEXT NOT NULL UNIQUE,
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Track which console user owns each instance
ALTER TABLE instances ADD COLUMN console_user_id UUID REFERENCES console_users(id) ON DELETE SET NULL;
