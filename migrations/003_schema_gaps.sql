-- Align eunha schema with Mastodon official schema.
-- Multi-tenancy columns (instance_id) and eunha-only tables are intentionally preserved.

-- ── accounts: add missing columns ────────────────────────────────────────────
ALTER TABLE accounts
    ADD COLUMN IF NOT EXISTS avatar_description       TEXT    NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS header_description       TEXT    NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS show_featured            BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS show_media               BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS show_media_replies       BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS collections_url          TEXT,
    ADD COLUMN IF NOT EXISTS feature_approval_policy  INTEGER NOT NULL DEFAULT 0;

-- ── user_roles: add missing columns ──────────────────────────────────────────
ALTER TABLE user_roles
    ADD COLUMN IF NOT EXISTS collection_limit  INTEGER NOT NULL DEFAULT 10,
    ADD COLUMN IF NOT EXISTS require_2fa       BOOLEAN NOT NULL DEFAULT false;

-- ── custom_emoji_categories: add featured_emoji_id + name uniqueness ─────────
ALTER TABLE custom_emoji_categories
    ADD COLUMN IF NOT EXISTS featured_emoji_id BIGINT REFERENCES custom_emojis(id) ON DELETE SET NULL;

CREATE UNIQUE INDEX IF NOT EXISTS index_custom_emoji_categories_on_name
    ON custom_emoji_categories(name);

-- ── quotes: make quoted_*_id nullable, fix indexes ───────────────────────────

-- Drop inline UNIQUE column constraints (unconditional) and the plain status_id index.
ALTER TABLE quotes DROP CONSTRAINT IF EXISTS quotes_activity_uri_key;
ALTER TABLE quotes DROP CONSTRAINT IF EXISTS quotes_approval_uri_key;
DROP INDEX IF EXISTS quotes_status_id_idx;

-- The referenced status/account may not be known yet for remote quotes.
ALTER TABLE quotes ALTER COLUMN quoted_status_id  DROP NOT NULL;
ALTER TABLE quotes ALTER COLUMN quoted_account_id DROP NOT NULL;

-- status_id is 1:1 with a quote (one status can only quote once).
CREATE UNIQUE INDEX IF NOT EXISTS index_quotes_on_status_id
    ON quotes(status_id);

-- activity_uri: unique but only when set.
CREATE UNIQUE INDEX IF NOT EXISTS index_quotes_on_activity_uri
    ON quotes(activity_uri) WHERE activity_uri IS NOT NULL;

-- approval_uri: non-unique index (Mastodon does not enforce uniqueness here).
CREATE INDEX IF NOT EXISTS index_quotes_on_approval_uri
    ON quotes(approval_uri) WHERE approval_uri IS NOT NULL;

-- Composite indexes for efficient lookups.
CREATE INDEX IF NOT EXISTS index_quotes_on_account_id_and_quoted_account_id_and_id
    ON quotes(account_id, quoted_account_id, id);

CREATE INDEX IF NOT EXISTS index_quotes_on_quoted_status_id_and_id
    ON quotes(quoted_status_id, id);

-- ── custom_filter_statuses: add unique constraint ─────────────────────────────
CREATE UNIQUE INDEX IF NOT EXISTS index_custom_filter_statuses_on_status_id_and_custom_filter_id
    ON custom_filter_statuses(status_id, custom_filter_id);

-- ── fasp_follow_recommendations: align with Mastodon ─────────────────────────
-- Mastodon's table only holds requesting/recommended account pairs; the extra
-- fasp_provider_id, account_id, and acct columns are not in the upstream schema.
DELETE FROM fasp_follow_recommendations
    WHERE requesting_account_id IS NULL OR recommended_account_id IS NULL;

ALTER TABLE fasp_follow_recommendations
    DROP COLUMN IF EXISTS fasp_provider_id,
    DROP COLUMN IF EXISTS account_id,
    DROP COLUMN IF EXISTS acct;

ALTER TABLE fasp_follow_recommendations
    ALTER COLUMN requesting_account_id  SET NOT NULL,
    ALTER COLUMN recommended_account_id SET NOT NULL;

-- Drop conditional indexes (they relied on columns now guaranteed NOT NULL).
DROP INDEX IF EXISTS index_fasp_follow_recommendations_on_requesting_account_id;
DROP INDEX IF EXISTS index_fasp_follow_recommendations_on_recommended_account_id;

CREATE INDEX IF NOT EXISTS index_fasp_follow_recommendations_on_requesting_account_id
    ON fasp_follow_recommendations(requesting_account_id);

CREATE INDEX IF NOT EXISTS index_fasp_follow_recommendations_on_recommended_account_id
    ON fasp_follow_recommendations(recommended_account_id);

-- ── email_subscriptions: new table ───────────────────────────────────────────
CREATE TABLE IF NOT EXISTS email_subscriptions (
    id                  BIGSERIAL PRIMARY KEY,
    account_id          BIGINT      NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    email               TEXT        NOT NULL,
    locale              TEXT        NOT NULL,
    confirmation_token  TEXT,
    confirmed_at        TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS index_email_subscriptions_on_account_id_and_email
    ON email_subscriptions(account_id, email);

CREATE UNIQUE INDEX IF NOT EXISTS index_email_subscriptions_on_confirmation_token
    ON email_subscriptions(confirmation_token) WHERE confirmation_token IS NOT NULL;

-- ── keypairs: new table ───────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS keypairs (
    id          BIGSERIAL PRIMARY KEY,
    account_id  BIGINT      NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    type        INTEGER     NOT NULL,
    uri         TEXT        NOT NULL,
    public_key  TEXT        NOT NULL,
    private_key TEXT,
    revoked     BOOLEAN     NOT NULL DEFAULT false,
    expires_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS index_keypairs_on_account_id ON keypairs(account_id);
CREATE UNIQUE INDEX IF NOT EXISTS index_keypairs_on_uri ON keypairs(uri);

-- ── tagged_objects: new table ─────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS tagged_objects (
    id          BIGSERIAL PRIMARY KEY,
    status_id   BIGINT      NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    ap_type     TEXT        NOT NULL,
    object_type TEXT,
    object_id   BIGINT,
    uri         TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS index_tagged_objects_on_object
    ON tagged_objects(object_type, object_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_on_status_id_object_type_object_id_tagged
    ON tagged_objects(status_id, object_type, object_id)
    WHERE object_type IS NOT NULL AND object_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS index_tagged_objects_on_status_id_and_uri
    ON tagged_objects(status_id, uri) WHERE uri IS NOT NULL;

-- ── collections + collection_items + collection_reports: new tables ───────────
CREATE TABLE IF NOT EXISTS collections (
    id                          BIGSERIAL PRIMARY KEY,
    account_id                  BIGINT      NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name                        TEXT        NOT NULL,
    description                 TEXT,
    description_html            TEXT,
    discoverable                BOOLEAN     NOT NULL,
    local                       BOOLEAN     NOT NULL,
    sensitive                   BOOLEAN     NOT NULL,
    item_count                  INTEGER     NOT NULL DEFAULT 0,
    original_number_of_items    INTEGER,
    language                    TEXT,
    tag_id                      BIGINT REFERENCES tags(id) ON DELETE SET NULL,
    uri                         TEXT,
    url                         TEXT,
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS index_collections_on_account_id ON collections(account_id);
CREATE INDEX IF NOT EXISTS index_collections_on_tag_id     ON collections(tag_id);
CREATE UNIQUE INDEX IF NOT EXISTS index_collections_on_uri ON collections(uri) WHERE uri IS NOT NULL;

CREATE TABLE IF NOT EXISTS collection_items (
    id                          BIGSERIAL PRIMARY KEY,
    collection_id               BIGINT      NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    account_id                  BIGINT REFERENCES accounts(id) ON DELETE SET NULL,
    uri                         TEXT,
    object_uri                  TEXT,
    approval_uri                TEXT,
    approval_last_verified_at   TIMESTAMPTZ,
    position                    INTEGER     NOT NULL DEFAULT 1,
    state                       INTEGER     NOT NULL DEFAULT 0,
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS index_collection_items_on_account_id_and_collection_id
    ON collection_items(account_id, collection_id);
CREATE UNIQUE INDEX IF NOT EXISTS index_collection_items_on_approval_uri
    ON collection_items(approval_uri) WHERE approval_uri IS NOT NULL;
CREATE INDEX IF NOT EXISTS index_collection_items_on_collection_id
    ON collection_items(collection_id);
CREATE INDEX IF NOT EXISTS index_collection_items_on_state
    ON collection_items(state) WHERE state = ANY(ARRAY[2, 3]);
CREATE UNIQUE INDEX IF NOT EXISTS index_collection_items_on_uri
    ON collection_items(uri) WHERE uri IS NOT NULL;

CREATE TABLE IF NOT EXISTS collection_reports (
    id              BIGSERIAL PRIMARY KEY,
    collection_id   BIGINT NOT NULL REFERENCES collections(id)  ON DELETE CASCADE,
    report_id       BIGINT NOT NULL REFERENCES reports(id)       ON DELETE CASCADE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS index_collection_reports_on_collection_id ON collection_reports(collection_id);
CREATE INDEX IF NOT EXISTS index_collection_reports_on_report_id     ON collection_reports(report_id);
