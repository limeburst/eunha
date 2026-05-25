-- Fixup applied after pg_restore from a Mastodon dump.
-- Variables: :old_domain (e.g. seoul.earth), :new_domain (e.g. eunha.social)

-- Remove orphan rows that reference deleted accounts (source DB may have them).
DELETE FROM account_aliases          WHERE account_id NOT IN (SELECT id FROM accounts);
DELETE FROM account_conversations    WHERE account_id NOT IN (SELECT id FROM accounts);
DELETE FROM account_domain_blocks    WHERE account_id NOT IN (SELECT id FROM accounts);
DELETE FROM account_migrations       WHERE account_id NOT IN (SELECT id FROM accounts);
DELETE FROM account_notes            WHERE account_id NOT IN (SELECT id FROM accounts)
                                        OR target_account_id NOT IN (SELECT id FROM accounts);
DELETE FROM account_pins             WHERE account_id NOT IN (SELECT id FROM accounts)
                                        OR target_account_id NOT IN (SELECT id FROM accounts);
DELETE FROM account_stats            WHERE account_id NOT IN (SELECT id FROM accounts);
DELETE FROM blocks                   WHERE account_id NOT IN (SELECT id FROM accounts)
                                        OR target_account_id NOT IN (SELECT id FROM accounts);
DELETE FROM bookmarks                WHERE account_id NOT IN (SELECT id FROM accounts);
DELETE FROM follows                  WHERE account_id NOT IN (SELECT id FROM accounts)
                                        OR target_account_id NOT IN (SELECT id FROM accounts);
DELETE FROM follow_requests          WHERE account_id NOT IN (SELECT id FROM accounts)
                                        OR target_account_id NOT IN (SELECT id FROM accounts);
DELETE FROM mutes                    WHERE account_id NOT IN (SELECT id FROM accounts)
                                        OR target_account_id NOT IN (SELECT id FROM accounts);
DELETE FROM notifications            WHERE account_id NOT IN (SELECT id FROM accounts)
                                        OR from_account_id NOT IN (SELECT id FROM accounts);

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

