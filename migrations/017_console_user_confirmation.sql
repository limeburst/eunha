ALTER TABLE console_users
  ADD COLUMN confirmed_at       TIMESTAMPTZ,
  ADD COLUMN confirmation_token TEXT UNIQUE;

-- Backfill: existing users are already confirmed
UPDATE console_users SET confirmed_at = created_at;
