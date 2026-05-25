-- Align eunha schema with Mastodon's column names/types.

-- Rename users.password_hash to match Mastodon's column name.
ALTER TABLE users RENAME COLUMN password_hash TO encrypted_password;

-- Replace media_type text column with a GENERATED column derived from
-- Mastodon's integer type column. pg_restore populates type; media_type
-- is computed automatically. No conversion needed in the migration fixup.
ALTER TABLE media_attachments DROP COLUMN media_type;
ALTER TABLE media_attachments
    ADD COLUMN media_type TEXT GENERATED ALWAYS AS (
        CASE "type"
            WHEN 0 THEN 'image'
            WHEN 1 THEN 'gifv'
            WHEN 2 THEN 'video'
            WHEN 3 THEN 'audio'
            ELSE 'unknown'
        END
    ) STORED;
