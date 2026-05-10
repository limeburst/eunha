ALTER TABLE users
  ADD COLUMN approved_at  TIMESTAMPTZ,
  ADD COLUMN reason       TEXT;

-- Backfill: all existing users are already approved
UPDATE users SET approved_at = COALESCE(confirmed_at, created_at);
