-- oauth_applications: migrate to mastodon-compatible column names
-- Mastodon uses uid/secret/redirect_uri; eunha added client_id/client_secret/redirect_uris.
-- Copy the active values into the mastodon-named columns then drop the eunha-specific ones.

UPDATE oauth_applications
SET uid = client_id, secret = client_secret, redirect_uri = redirect_uris;

-- Replace partial unique index (WHERE uid <> '') with a full unique constraint.
DROP INDEX IF EXISTS index_oauth_applications_on_uid;
ALTER TABLE oauth_applications
    DROP CONSTRAINT IF EXISTS oauth_applications_client_id_key;
ALTER TABLE oauth_applications
    ADD CONSTRAINT oauth_applications_uid_key UNIQUE (uid);

-- Drop the empty-string defaults that were placeholders before this migration.
ALTER TABLE oauth_applications
    ALTER COLUMN uid     DROP DEFAULT,
    ALTER COLUMN secret  DROP DEFAULT,
    ALTER COLUMN redirect_uri SET DEFAULT 'urn:ietf:wg:oauth:2.0:oob';

ALTER TABLE oauth_applications
    DROP COLUMN client_id,
    DROP COLUMN client_secret,
    DROP COLUMN redirect_uris;

-- notifications: rename notification_type → type (mastodon's column name).
-- The type column already exists but was always NULL; populate it now.

UPDATE notifications SET "type" = notification_type WHERE "type" IS NULL;
ALTER TABLE notifications ALTER COLUMN "type" SET NOT NULL;
ALTER TABLE notifications DROP COLUMN notification_type;

-- markers: drop the eunha-specific version column; lock_version is mastodon's name.

UPDATE markers SET lock_version = version;
ALTER TABLE markers DROP COLUMN version;
