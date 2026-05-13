ALTER TABLE users
  ADD COLUMN IF NOT EXISTS password_reset_token TEXT,
  ADD COLUMN IF NOT EXISTS password_reset_sent_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_users_password_reset_token ON users (password_reset_token)
  WHERE password_reset_token IS NOT NULL;
