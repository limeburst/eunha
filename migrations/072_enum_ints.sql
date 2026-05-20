-- Convert text enum columns to integer to match Mastodon's canonical schema.
--
-- Integer mappings:
--   statuses.visibility:        public=0 unlisted=1 private=2 direct=3
--   domain_blocks.severity:     noop=0 silence=1 suspend=2
--   ip_blocks.severity:         sign_up_requires_approval=0 sign_up_block=1 block=2
--   lists.replies_policy:       followed=0 list=1 none=2
--   reports.category:           other=0 spam=1 violation=2
--   account_warnings.action:    none=0 disable=1 mark_statuses_as_sensitive=2 silence=3 suspend=4 delete_statuses=5
--   custom_filters.action:      warn=0 hide=1
--   quotes.state:               pending=0 accepted=1 rejected=2 revoked=3

-- ── 1. statuses.visibility ────────────────────────────────────────────────
-- Drop dependent partial indexes and check constraint before type change.
DROP INDEX IF EXISTS statuses_public;
DROP INDEX IF EXISTS statuses_public_timeline;
ALTER TABLE statuses DROP CONSTRAINT IF EXISTS statuses_visibility_check;

ALTER TABLE statuses ALTER COLUMN visibility DROP DEFAULT;
ALTER TABLE statuses ALTER COLUMN visibility TYPE INTEGER
    USING CASE visibility
        WHEN 'public' THEN 0 WHEN 'unlisted' THEN 1
        WHEN 'private' THEN 2 WHEN 'direct' THEN 3 ELSE 0
    END;
ALTER TABLE statuses ALTER COLUMN visibility SET DEFAULT 0;
ALTER TABLE statuses ADD CONSTRAINT statuses_visibility_check
    CHECK (visibility IN (0, 1, 2, 3));

CREATE INDEX statuses_public ON statuses (id DESC)
    WHERE visibility = 0 AND deleted_at IS NULL AND reblog_of_id IS NULL;
CREATE INDEX statuses_public_timeline ON statuses (instance_id, id DESC)
    WHERE visibility = 0 AND deleted_at IS NULL AND reblog_of_id IS NULL
      AND (NOT reply OR in_reply_to_account_id = account_id);

-- ── 2. domain_blocks.severity ─────────────────────────────────────────────
ALTER TABLE domain_blocks ALTER COLUMN severity DROP DEFAULT;
ALTER TABLE domain_blocks ALTER COLUMN severity TYPE INTEGER
    USING CASE severity
        WHEN 'noop' THEN 0 WHEN 'silence' THEN 1 WHEN 'suspend' THEN 2 ELSE 0
    END;
ALTER TABLE domain_blocks ALTER COLUMN severity SET DEFAULT 0;

-- ── 3. ip_blocks.severity ─────────────────────────────────────────────────
ALTER TABLE ip_blocks ALTER COLUMN severity DROP DEFAULT;
ALTER TABLE ip_blocks ALTER COLUMN severity TYPE INTEGER
    USING CASE severity
        WHEN 'sign_up_requires_approval' THEN 0 WHEN 'sign_up_block' THEN 1 WHEN 'block' THEN 2 ELSE 0
    END;
ALTER TABLE ip_blocks ALTER COLUMN severity SET DEFAULT 0;

-- ── 4. lists.replies_policy ───────────────────────────────────────────────
ALTER TABLE lists ALTER COLUMN replies_policy DROP DEFAULT;
ALTER TABLE lists ALTER COLUMN replies_policy TYPE INTEGER
    USING CASE replies_policy
        WHEN 'followed' THEN 0 WHEN 'list' THEN 1 WHEN 'none' THEN 2 ELSE 1
    END;
ALTER TABLE lists ALTER COLUMN replies_policy SET DEFAULT 1;

-- ── 5. reports.category ───────────────────────────────────────────────────
ALTER TABLE reports ALTER COLUMN category DROP DEFAULT;
ALTER TABLE reports ALTER COLUMN category TYPE INTEGER
    USING CASE category
        WHEN 'other' THEN 0 WHEN 'spam' THEN 1 WHEN 'violation' THEN 2 ELSE 0
    END;
ALTER TABLE reports ALTER COLUMN category SET DEFAULT 0;

-- ── 6. account_warnings.action ───────────────────────────────────────────
ALTER TABLE account_warnings ALTER COLUMN action DROP DEFAULT;
ALTER TABLE account_warnings ALTER COLUMN action TYPE INTEGER
    USING CASE action
        WHEN 'none' THEN 0 WHEN 'disable' THEN 1
        WHEN 'mark_statuses_as_sensitive' THEN 2 WHEN 'silence' THEN 3
        WHEN 'suspend' THEN 4 WHEN 'delete_statuses' THEN 5 ELSE 0
    END;
ALTER TABLE account_warnings ALTER COLUMN action SET DEFAULT 0;

-- ── 7. custom_filters.action ──────────────────────────────────────────────
ALTER TABLE custom_filters ALTER COLUMN action DROP DEFAULT;
ALTER TABLE custom_filters ALTER COLUMN action TYPE INTEGER
    USING CASE action
        WHEN 'warn' THEN 0 WHEN 'hide' THEN 1 ELSE 0
    END;
ALTER TABLE custom_filters ALTER COLUMN action SET DEFAULT 0;

-- ── 8. quotes.state ───────────────────────────────────────────────────────
ALTER TABLE quotes ALTER COLUMN state DROP DEFAULT;
ALTER TABLE quotes ALTER COLUMN state TYPE INTEGER
    USING CASE state
        WHEN 'pending' THEN 0 WHEN 'accepted' THEN 1
        WHEN 'rejected' THEN 2 WHEN 'revoked' THEN 3 ELSE 0
    END;
ALTER TABLE quotes ALTER COLUMN state SET DEFAULT 0;
