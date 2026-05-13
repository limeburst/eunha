-- Backfill url/uri for local accounts that were migrated with empty values.
UPDATE accounts a
SET
    url = 'https://' || i.domain || '/@' || a.username,
    uri = 'https://' || i.domain || '/users/' || a.username,
    inbox_url = CASE WHEN a.inbox_url = '' THEN 'https://' || i.domain || '/users/' || a.username || '/inbox' ELSE a.inbox_url END,
    outbox_url = CASE WHEN a.outbox_url = '' THEN 'https://' || i.domain || '/users/' || a.username || '/outbox' ELSE a.outbox_url END,
    shared_inbox_url = CASE WHEN a.shared_inbox_url IS NULL THEN 'https://' || i.domain || '/inbox' ELSE a.shared_inbox_url END
FROM instances i
WHERE a.instance_id = i.id
  AND a.domain IS NULL
  AND a.url = '';

-- Repair mention href="" in status content caused by the empty account URLs above.
UPDATE statuses s
SET content = regexp_replace(
    s.content,
    E'<a href="" class="u-url mention">@<span>([^<]+)</span></a>',
    '<a href="https://' || i.domain || E'/@\\1" class="u-url mention">@<span>\\1</span></a>',
    'g'
)
FROM instances i
WHERE s.instance_id = i.id
  AND s.content LIKE '%href=""%'
  AND s.content LIKE '%u-url mention%'
  AND s.deleted_at IS NULL;
