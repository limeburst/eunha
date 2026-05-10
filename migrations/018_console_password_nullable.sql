-- Password is now set on the confirmation page, not at signup time.
ALTER TABLE console_users ALTER COLUMN password_hash DROP NOT NULL;
