-- Migration 061: Per-user default quote policy preference
ALTER TABLE users ADD COLUMN default_quote_policy TEXT NOT NULL DEFAULT 'public';
