-- Align eunha schema with Mastodon's schema.rb (version 2026-05-05).
-- Multi-tenancy columns (instance_id), eunha-only tables, and columns
-- deeply embedded in eunha code are intentionally preserved.

-- ── accounts: fix id_scheme default ──────────────────────────────────────────
-- Mastodon: id_scheme default: 1; eunha had no default.
ALTER TABLE accounts ALTER COLUMN id_scheme SET DEFAULT 1;

-- ── announcements: fix published default ──────────────────────────────────────
-- Mastodon: published DEFAULT false (admin must explicitly publish).
ALTER TABLE announcements ALTER COLUMN published SET DEFAULT false;

-- ── notification_requests: fix notifications_count default ───────────────────
-- Mastodon: notifications_count DEFAULT 0; eunha had DEFAULT 1.
-- Code always supplies an explicit value in INSERT, so no data hazard.
ALTER TABLE notification_requests ALTER COLUMN notifications_count SET DEFAULT 0;

-- ── lists: fix replies_policy default ─────────────────────────────────────────
-- Mastodon: replies_policy DEFAULT 0; eunha had DEFAULT 1.
ALTER TABLE lists ALTER COLUMN replies_policy SET DEFAULT 0;

-- ── reports: align rule_ids type ──────────────────────────────────────────────
-- Mastodon uses BIGINT[]; eunha had INTEGER[].
ALTER TABLE reports ALTER COLUMN rule_ids TYPE BIGINT[] USING rule_ids::BIGINT[];

-- ── preview_cards: add unverified_author_account_id ──────────────────────────
ALTER TABLE preview_cards
    ADD COLUMN IF NOT EXISTS unverified_author_account_id
        BIGINT REFERENCES accounts(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS index_preview_cards_on_unverified_author_account_id_and_id
    ON preview_cards(unverified_author_account_id, id)
    WHERE unverified_author_account_id IS NOT NULL;

-- ── tags: replace case-sensitive unique with case-insensitive btree ───────────
-- Mastodon: unique index on lower(name) text_pattern_ops.
-- eunha had UNIQUE constraint on the raw name column.
ALTER TABLE tags DROP CONSTRAINT IF EXISTS tags_name_key;
CREATE UNIQUE INDEX IF NOT EXISTS index_tags_on_name_lower_btree
    ON tags(lower(name) text_pattern_ops);

-- ── accounts_tags: swap primary key column order to match Mastodon ────────────
-- Mastodon PK: (tag_id, account_id) with a secondary index on (account_id, tag_id).
-- eunha PK was: (account_id, tag_id) with a secondary index on tag_id.
ALTER TABLE accounts_tags DROP CONSTRAINT IF EXISTS accounts_tags_pkey;
DROP INDEX IF EXISTS index_accounts_tags_on_tag_id;
ALTER TABLE accounts_tags ADD PRIMARY KEY (tag_id, account_id);
CREATE INDEX IF NOT EXISTS index_accounts_tags_on_account_id_and_tag_id
    ON accounts_tags(account_id, tag_id);

-- ── statuses_tags: swap primary key column order to match Mastodon ────────────
-- Mastodon PK: (tag_id, status_id) with a secondary index on status_id.
-- eunha PK was: (status_id, tag_id) with an index on tag_id.
ALTER TABLE statuses_tags DROP CONSTRAINT IF EXISTS statuses_tags_pkey;
DROP INDEX IF EXISTS statuses_tags_by_tag;
ALTER TABLE statuses_tags ADD PRIMARY KEY (tag_id, status_id);
CREATE INDEX IF NOT EXISTS index_statuses_tags_on_status_id
    ON statuses_tags(status_id);

-- ── relationship_severance_events: add missing index ─────────────────────────
CREATE INDEX IF NOT EXISTS index_relationship_severance_events_on_type_and_target_name
    ON relationship_severance_events(type, target_name);

-- ── software_updates: add unique index on version ────────────────────────────
CREATE UNIQUE INDEX IF NOT EXISTS index_software_updates_on_version
    ON software_updates(version);

-- ── bulk_imports: add partial index for unconfirmed state ────────────────────
CREATE INDEX IF NOT EXISTS index_bulk_imports_unconfirmed
    ON bulk_imports(id) WHERE state = 0;

-- ── scheduled_statuses: add index on scheduled_at ────────────────────────────
CREATE INDEX IF NOT EXISTS index_scheduled_statuses_on_scheduled_at
    ON scheduled_statuses(scheduled_at);

-- ── site_uploads: add unique index on var ────────────────────────────────────
CREATE UNIQUE INDEX IF NOT EXISTS index_site_uploads_on_var
    ON site_uploads(var);

-- ── preview_card_providers: add unique index on domain ───────────────────────
CREATE UNIQUE INDEX IF NOT EXISTS index_preview_card_providers_on_domain
    ON preview_card_providers(domain);

-- ── identities: add unique index on (uid, provider) ──────────────────────────
CREATE UNIQUE INDEX IF NOT EXISTS index_identities_on_uid_and_provider
    ON identities(uid, provider);

-- ── Missing target_account_id indexes ────────────────────────────────────────

CREATE INDEX IF NOT EXISTS index_blocks_on_target_account_id
    ON blocks(target_account_id);

CREATE INDEX IF NOT EXISTS index_mutes_on_target_account_id
    ON mutes(target_account_id);

CREATE INDEX IF NOT EXISTS index_account_notes_on_target_account_id
    ON account_notes(target_account_id);

CREATE INDEX IF NOT EXISTS index_account_pins_on_target_account_id
    ON account_pins(target_account_id);

CREATE INDEX IF NOT EXISTS index_account_moderation_notes_on_account_id
    ON account_moderation_notes(account_id);

CREATE INDEX IF NOT EXISTS index_account_moderation_notes_on_target_account_id
    ON account_moderation_notes(target_account_id);

-- ── notification_requests: add missing last_status_id index ──────────────────
CREATE INDEX IF NOT EXISTS index_notification_requests_on_last_status_id
    ON notification_requests(last_status_id) WHERE last_status_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS index_notification_requests_on_from_account_id
    ON notification_requests(from_account_id);

-- ── admin_action_logs: add target_type/target_id composite index ──────────────
CREATE INDEX IF NOT EXISTS index_admin_action_logs_on_target_type_and_target_id
    ON admin_action_logs(target_type, target_id)
    WHERE target_type IS NOT NULL AND target_id IS NOT NULL;

-- ── statuses: add text_pattern_ops opclass to uri index ──────────────────────
-- Mastodon uses text_pattern_ops for LIKE-based lookups on URI.
DROP INDEX IF EXISTS statuses_uri_unique;
CREATE UNIQUE INDEX IF NOT EXISTS index_statuses_on_uri
    ON statuses(uri text_pattern_ops)
    WHERE uri IS NOT NULL;

-- ── fasp_providers: remove base64 key columns (not in Mastodon) ───────────────
-- Mastodon only stores PEM-encoded keys; the base64 variants were extra.
ALTER TABLE fasp_providers
    DROP COLUMN IF EXISTS provider_public_key_base64,
    DROP COLUMN IF EXISTS server_private_key_base64,
    DROP COLUMN IF EXISTS server_public_key_base64;

-- ── fasp_debug_callbacks: remove payload column (not in Mastodon) ────────────
ALTER TABLE fasp_debug_callbacks
    DROP COLUMN IF EXISTS payload;

-- ── fasp_subscriptions: remove active column (not in Mastodon) ───────────────
ALTER TABLE fasp_subscriptions
    DROP COLUMN IF EXISTS active;

-- ── account_summaries: materialized view ─────────────────────────────────────
CREATE MATERIALIZED VIEW IF NOT EXISTS account_summaries AS
SELECT
    accounts.id AS account_id,
    mode() WITHIN GROUP (ORDER BY t0.language)  AS language,
    mode() WITHIN GROUP (ORDER BY t0.sensitive) AS sensitive
FROM accounts
CROSS JOIN LATERAL (
    SELECT statuses.language, statuses.sensitive
    FROM statuses
    WHERE statuses.account_id = accounts.id
      AND statuses.deleted_at IS NULL
      AND statuses.reblog_of_id IS NULL
    ORDER BY statuses.id DESC
    LIMIT 20
) t0
WHERE accounts.suspended_at      IS NULL
  AND accounts.silenced_at        IS NULL
  AND accounts.moved_to_account_id IS NULL
  AND accounts.discoverable       = true
  AND accounts.locked             = false
GROUP BY accounts.id;

CREATE UNIQUE INDEX IF NOT EXISTS index_account_summaries_on_account_id
    ON account_summaries(account_id);

CREATE INDEX IF NOT EXISTS idx_on_account_id_language_sensitive_250461e1eb
    ON account_summaries(account_id, language, sensitive);

-- ── global_follow_recommendations: materialized view ─────────────────────────
CREATE MATERIALIZED VIEW IF NOT EXISTS global_follow_recommendations AS
SELECT account_id,
       sum(rank)         AS rank,
       array_agg(reason) AS reason
FROM (
    -- Signal 1: most followed by recently active users
    SELECT
        account_summaries.account_id,
        (count(follows.id)::numeric / (1.0 + count(follows.id)::numeric)) AS rank,
        'most_followed'::text AS reason
    FROM follows
    JOIN account_summaries ON account_summaries.account_id = follows.target_account_id
    JOIN users             ON users.account_id             = follows.account_id
    WHERE users.current_sign_in_at >= now() - INTERVAL 'P30D'
      AND account_summaries.sensitive = false
      AND NOT EXISTS (
          SELECT 1 FROM follow_recommendation_suppressions frs
          WHERE frs.account_id = follows.target_account_id
      )
    GROUP BY account_summaries.account_id
    HAVING count(follows.id) >= 5

    UNION ALL

    -- Signal 2: most interactions in last 30 days
    SELECT
        account_summaries.account_id,
        (sum(status_stats.reblogs_count + status_stats.favourites_count)
             / (1.0 + sum(status_stats.reblogs_count + status_stats.favourites_count))) AS rank,
        'most_interactions'::text AS reason
    FROM status_stats
    JOIN statuses       ON statuses.id           = status_stats.status_id
    JOIN account_summaries ON account_summaries.account_id = statuses.account_id
    WHERE statuses.id >= (date_part('epoch', now() - INTERVAL 'P30D') * 1000)::bigint << 16
      AND account_summaries.sensitive = false
      AND NOT EXISTS (
          SELECT 1 FROM follow_recommendation_suppressions frs
          WHERE frs.account_id = statuses.account_id
      )
    GROUP BY account_summaries.account_id
    HAVING sum(status_stats.reblogs_count + status_stats.favourites_count) >= 5
) t0
GROUP BY account_id
ORDER BY sum(rank) DESC;

CREATE UNIQUE INDEX IF NOT EXISTS index_global_follow_recommendations_on_account_id
    ON global_follow_recommendations(account_id);
