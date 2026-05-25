-- Fixup applied after pg_restore from a Mastodon dump.
-- Variables: :old_domain (e.g. seoul.earth), :new_domain (e.g. eunha.social)

-- Remove the Mastodon instance-actor (synthetic account, not a real user).
DELETE FROM accounts WHERE username = :'old_domain' AND domain IS NULL;

-- Rewrite local account URLs to the new domain.
UPDATE accounts
SET uri        = replace(uri,        'https://' || :'old_domain', 'https://' || :'new_domain'),
    url        = replace(url,        'https://' || :'old_domain', 'https://' || :'new_domain'),
    inbox_url  = replace(inbox_url,  'https://' || :'old_domain', 'https://' || :'new_domain'),
    outbox_url = replace(outbox_url, 'https://' || :'old_domain', 'https://' || :'new_domain')
WHERE domain IS NULL;

-- Rewrite local status URLs.
UPDATE statuses
SET uri = replace(uri, 'https://' || :'old_domain', 'https://' || :'new_domain'),
    url = replace(url, 'https://' || :'old_domain', 'https://' || :'new_domain')
WHERE uri LIKE 'https://' || :'old_domain' || '%';

-- Copy password hash from Mastodon's column to eunha's column.
UPDATE users
SET password_hash = encrypted_password
WHERE (password_hash = '' OR password_hash IS NULL)
  AND encrypted_password IS NOT NULL
  AND encrypted_password != '';

-- Derive media_type (eunha string) from Mastodon's integer type column.
UPDATE media_attachments SET media_type = CASE type
    WHEN 0 THEN 'image'
    WHEN 1 THEN 'gifv'
    WHEN 2 THEN 'video'
    WHEN 3 THEN 'audio'
    ELSE 'unknown'
END;

-- Derive status_id in notifications from Mastodon's polymorphic activity columns.
UPDATE notifications n SET status_id = (
    CASE n.activity_type
        WHEN 'Status'    THEN n.activity_id
        WHEN 'Mention'   THEN (SELECT status_id FROM mentions   WHERE id = n.activity_id)
        WHEN 'Favourite' THEN (SELECT status_id FROM favourites WHERE id = n.activity_id)
        WHEN 'Poll'      THEN (SELECT status_id FROM polls      WHERE id = n.activity_id)
        ELSE NULL
    END
)
WHERE n.activity_id IS NOT NULL AND n.status_id IS NULL;
