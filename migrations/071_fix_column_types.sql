-- Fix column type differences between eunha and Mastodon's canonical schema.
-- Intentional differences preserved: UUIDs for user/invite IDs, TIMESTAMPTZ,
-- TEXT vs varchar, text enums (visibility, severity, etc.) for Rust compatibility.

-- ── 1. fasp_subscriptions.category: integer → text ───────────────────────────
-- Mastodon stores category as varchar (e.g. "follow_recommendations", "content").
-- eunha had integer; no rows exist, so this is a simple type change.
ALTER TABLE fasp_subscriptions ALTER COLUMN category TYPE TEXT USING category::TEXT;
ALTER TABLE fasp_subscriptions ALTER COLUMN category DROP DEFAULT;

-- ── 2. ip_blocks.ip: text → inet ─────────────────────────────────────────────
-- Mastodon uses inet type for proper IP/CIDR validation and indexing.
ALTER TABLE ip_blocks ALTER COLUMN ip TYPE INET USING ip::INET;
ALTER TABLE ip_blocks ALTER COLUMN ip SET DEFAULT '0.0.0.0';

-- ── 3. polls.options: jsonb → text[] ─────────────────────────────────────────
-- Mastodon stores poll option titles as a varchar[] array.
-- eunha stored them as jsonb objects [{title: ..., votes_count: ...}].
-- Extract just the titles; vote counts are computed live from poll_votes.
-- PostgreSQL USING clause does not allow subqueries, so use a staging column.
ALTER TABLE polls ADD COLUMN options_new TEXT[];
UPDATE polls SET options_new = ARRAY(
    SELECT elem->>'title'
    FROM jsonb_array_elements(options) WITH ORDINALITY AS t(elem, ord)
    ORDER BY t.ord
);
ALTER TABLE polls DROP COLUMN options;
ALTER TABLE polls RENAME COLUMN options_new TO options;
ALTER TABLE polls ALTER COLUMN options SET NOT NULL;
ALTER TABLE polls ALTER COLUMN options SET DEFAULT '{}';

-- ── 4. polls.cached_tallies: nullable → NOT NULL DEFAULT '{}' ────────────────
-- Mastodon has cached_tallies bigint[] NOT NULL DEFAULT '{}'.
UPDATE polls SET cached_tallies = '{}' WHERE cached_tallies IS NULL;
ALTER TABLE polls ALTER COLUMN cached_tallies SET NOT NULL;
ALTER TABLE polls ALTER COLUMN cached_tallies SET DEFAULT '{}';

-- ── 5. markers.last_read_id: text → bigint ────────────────────────────────────
-- Mastodon stores last_read_id as bigint. eunha used text (empty string as sentinel).
-- Drop the text default first, then change type, then set bigint default.
ALTER TABLE markers ALTER COLUMN last_read_id DROP DEFAULT;
ALTER TABLE markers ALTER COLUMN last_read_id TYPE BIGINT
    USING CASE WHEN last_read_id = '' THEN 0 ELSE last_read_id::BIGINT END;
ALTER TABLE markers ALTER COLUMN last_read_id SET DEFAULT 0;
