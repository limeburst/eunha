-- Drop the eunha-specific status_id column from notifications.
-- status_id is now derived at read time from activity_type / activity_id,
-- which are Mastodon's original polymorphic association columns.
-- New notifications created by eunha store activity_type = 'Status',
-- activity_id = <status id> for status-related types.
ALTER TABLE notifications DROP COLUMN status_id;
