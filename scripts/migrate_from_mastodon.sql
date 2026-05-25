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

