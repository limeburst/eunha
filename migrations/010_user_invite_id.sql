ALTER TABLE users ADD COLUMN invite_id UUID REFERENCES invites(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS users_by_invite ON users(invite_id) WHERE invite_id IS NOT NULL;
