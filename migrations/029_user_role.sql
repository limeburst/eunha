-- Add role column to users table for admin API access control.
-- 'user' (default), 'moderator', 'admin'
ALTER TABLE users ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'user';

-- The first user per instance (created via create_instance) becomes admin.
-- We identify them by being the oldest confirmed user per instance.
UPDATE users u
SET role = 'admin'
WHERE u.id IN (
    SELECT DISTINCT ON (instance_id) id
    FROM users
    ORDER BY instance_id, created_at ASC
);
