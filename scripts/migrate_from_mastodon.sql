-- Fixup applied after pg_restore from a Mastodon dump.
-- Variables: :old_domain (e.g. seoul.earth), :new_domain (e.g. eunha.social)

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

-- Populate columns that are NOT NULL in eunha but absent from Mastodon v4.5.10 dumps.
-- pg_restore does not set these; run this after restore.

-- custom_emojis.image_url: eunha stores a local URL; fall back to remote if local is absent.
UPDATE custom_emojis
SET image_url = COALESCE(NULLIF(image_remote_url, ''), image_file_name)
WHERE image_url IS NULL OR image_url = '';

-- markers.account_id: derive from the users row that owns this marker.
UPDATE markers m
SET account_id = u.account_id
FROM users u
WHERE u.id = m.user_id
  AND m.account_id IS NULL;

-- oauth_access_grants.expires_at: compute from created_at + expires_in seconds.
UPDATE oauth_access_grants
SET expires_at = created_at + (expires_in || ' seconds')::interval
WHERE expires_at IS NULL;

-- users.email_normalized: lower-cased email, matching Mastodon's normalisation.
UPDATE users
SET email_normalized = lower(email)
WHERE email_normalized IS NULL OR email_normalized = '';

-- web_push_subscriptions.account_id: derive from the users row that owns this subscription.
UPDATE web_push_subscriptions wps
SET account_id = u.account_id
FROM users u
WHERE u.id = wps.user_id
  AND wps.account_id IS NULL;
