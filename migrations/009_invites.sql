CREATE TABLE IF NOT EXISTS invites (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id UUID NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    code        TEXT NOT NULL UNIQUE,
    created_by  UUID REFERENCES accounts(id) ON DELETE SET NULL,
    max_uses    INT,            -- NULL = unlimited
    uses        INT NOT NULL DEFAULT 0,
    expires_at  TIMESTAMPTZ,   -- NULL = never expires
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS invites_by_instance ON invites(instance_id);
CREATE INDEX IF NOT EXISTS invites_by_code     ON invites(code);
