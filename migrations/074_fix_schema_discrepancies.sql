-- Fix schema discrepancies between eunha and Mastodon's canonical schema.
-- Reference: mastodon_src database comparison.
-- Intentional eunha differences preserved: UUIDs, TIMESTAMPTZ, TEXT vs varchar,
-- multi-tenancy columns, account_id-based ownership instead of user_id.

-- ── 1. account_aliases.acct: nullable → NOT NULL DEFAULT '' ─────────────────
UPDATE account_aliases SET acct = '' WHERE acct IS NULL;
ALTER TABLE account_aliases
    ALTER COLUMN acct SET NOT NULL,
    ALTER COLUMN acct SET DEFAULT '';

-- ── 2. accounts.shared_inbox_url: nullable → NOT NULL ───────────────────────
-- Migration 067 incorrectly dropped NOT NULL; 069 didn't restore it.
UPDATE accounts SET shared_inbox_url = '' WHERE shared_inbox_url IS NULL;
ALTER TABLE accounts ALTER COLUMN shared_inbox_url SET NOT NULL;

-- ── 3. conversation_mutes: restructure to match Mastodon ────────────────────
-- Mastodon keys mutes by conversation_id NOT NULL, not status_id.
-- Populate conversation_id from the muted status's conversation.
UPDATE conversation_mutes cm
SET conversation_id = s.conversation_id
FROM statuses s
WHERE s.id = cm.status_id AND cm.conversation_id IS NULL;

-- Delete orphaned rows (muted status deleted, conversation_id still null).
DELETE FROM conversation_mutes WHERE conversation_id IS NULL;

-- Make conversation_id the primary NOT NULL key.
ALTER TABLE conversation_mutes ALTER COLUMN conversation_id SET NOT NULL;

-- Drop old status_id unique constraint and FK (status_id not in Mastodon schema).
ALTER TABLE conversation_mutes DROP CONSTRAINT IF EXISTS conversation_mutes_account_id_status_id_key;
ALTER TABLE conversation_mutes DROP CONSTRAINT IF EXISTS conversation_mutes_status_id_fkey;
ALTER TABLE conversation_mutes DROP COLUMN IF EXISTS status_id;
ALTER TABLE conversation_mutes DROP COLUMN IF EXISTS created_at;

-- Replace partial index with proper unique constraint (matches Mastodon).
DROP INDEX IF EXISTS index_conversation_mutes_on_account_id_and_conversation_id;
ALTER TABLE conversation_mutes ADD UNIQUE (account_id, conversation_id);

-- ── 4. generated_annual_reports.data: nullable → NOT NULL DEFAULT '{}' ──────
ALTER TABLE generated_annual_reports
    ALTER COLUMN data SET NOT NULL,
    ALTER COLUMN data SET DEFAULT '{}';

-- ── 5. ip_blocks.comment: nullable → NOT NULL DEFAULT '' ────────────────────
ALTER TABLE ip_blocks
    ALTER COLUMN comment SET NOT NULL,
    ALTER COLUMN comment SET DEFAULT '';

-- ── 6. media_attachments.remote_url: nullable → NOT NULL DEFAULT '' ─────────
UPDATE media_attachments SET remote_url = '' WHERE remote_url IS NULL;
ALTER TABLE media_attachments
    ALTER COLUMN remote_url SET NOT NULL,
    ALTER COLUMN remote_url SET DEFAULT '';

-- ── 7. media_attachments.type: nullable → NOT NULL DEFAULT 0 ────────────────
UPDATE media_attachments SET type = 0 WHERE type IS NULL;
ALTER TABLE media_attachments
    ALTER COLUMN type SET NOT NULL,
    ALTER COLUMN type SET DEFAULT 0;

-- ── 8. rule_translations: add unique index on (rule_id, language) ───────────
-- Mastodon has UNIQUE (rule_id, language); eunha only has btree on rule_id.
-- Also drop locale column (not in Mastodon; duplicated by language).
CREATE UNIQUE INDEX IF NOT EXISTS index_rule_translations_on_rule_id_and_language
    ON rule_translations(rule_id, language);
ALTER TABLE rule_translations DROP COLUMN IF EXISTS locale;

-- ── 9. statuses.updated_at: nullable → NOT NULL DEFAULT now() ───────────────
UPDATE statuses SET updated_at = created_at WHERE updated_at IS NULL;
ALTER TABLE statuses
    ALTER COLUMN updated_at SET NOT NULL,
    ALTER COLUMN updated_at SET DEFAULT now();
