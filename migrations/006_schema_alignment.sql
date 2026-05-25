-- Align eunha schema with Mastodon's column names/types.

-- users: migration 004 added encrypted_password as a stub column (default '').
-- Copy actual hashes from password_hash then drop the eunha-specific column.
UPDATE users SET encrypted_password = password_hash
    WHERE encrypted_password = '' AND password_hash IS NOT NULL AND password_hash != '';
ALTER TABLE users DROP COLUMN password_hash;

-- media_attachments: drop the eunha-specific text media_type column (including its
-- check constraint), then re-add it as a GENERATED column derived from the integer
-- type column that Mastodon uses. pg_restore will populate type; media_type is
-- computed automatically — no conversion step needed in the migration fixup.
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
