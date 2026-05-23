-- Drop all multitenancy infrastructure: instance_id columns, related indexes/constraints,
-- and the instances table itself. Eunha becomes a single-tenant server.

-- ── 1. Drop indexes that include instance_id ─────────────────────────────────

DROP INDEX accounts_by_instance;
DROP INDEX invites_by_instance;
DROP INDEX statuses_by_instance;
DROP INDEX statuses_public_timeline;

-- ── 2. Drop named constraint that includes instance_id ───────────────────────

ALTER TABLE accounts DROP CONSTRAINT accounts_local_unique;

-- ── 3. Drop instance_id columns ──────────────────────────────────────────────
-- Dropping a column automatically removes any FK constraints, unnamed UNIQUE
-- constraints, and indexes that reference it.

ALTER TABLE oauth_applications DROP COLUMN instance_id;
ALTER TABLE accounts            DROP COLUMN instance_id;
ALTER TABLE invites             DROP COLUMN instance_id;
ALTER TABLE users               DROP COLUMN instance_id;
ALTER TABLE pending_signups     DROP COLUMN instance_id;
ALTER TABLE conversations       DROP COLUMN instance_id;
ALTER TABLE statuses            DROP COLUMN instance_id;
ALTER TABLE custom_emojis       DROP COLUMN instance_id;
ALTER TABLE announcements       DROP COLUMN instance_id;
ALTER TABLE rules               DROP COLUMN instance_id;
ALTER TABLE terms_of_services   DROP COLUMN instance_id;
ALTER TABLE webhooks            DROP COLUMN instance_id;

-- ── 4. Recreate unique constraints without instance_id ───────────────────────

ALTER TABLE accounts
    ADD CONSTRAINT accounts_local_unique
    UNIQUE NULLS NOT DISTINCT (username, domain);

ALTER TABLE users         ADD UNIQUE (email_normalized);
ALTER TABLE pending_signups ADD UNIQUE (email_normalized);
ALTER TABLE custom_emojis  ADD UNIQUE (shortcode);

-- ── 5. Recreate statuses_public_timeline without instance_id ─────────────────

CREATE INDEX statuses_public_timeline
    ON statuses(id DESC)
    WHERE visibility = 0
      AND deleted_at IS NULL
      AND reblog_of_id IS NULL
      AND (NOT reply OR in_reply_to_account_id = account_id);

-- ── 6. Drop the instances table ───────────────────────────────────────────────

DROP TABLE instances CASCADE;
