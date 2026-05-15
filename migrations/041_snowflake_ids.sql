-- IDs for statuses and media_attachments are now generated in application
-- code using Mastodon-compatible Snowflake IDs (48-bit ms timestamp + 16-bit
-- sequence). Drop the plain auto-increment defaults so accidental inserts
-- without an explicit ID fail loudly instead of silently getting a sequential ID.
ALTER TABLE statuses ALTER COLUMN id DROP DEFAULT;
ALTER TABLE media_attachments ALTER COLUMN id DROP DEFAULT;
DROP SEQUENCE IF EXISTS status_id_seq;
DROP SEQUENCE IF EXISTS media_id_seq;
