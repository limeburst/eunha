-- Create tables present in Mastodon but missing from eunha.

-- ── account_conversations ────────────────────────────────────────────────────
-- Mastodon's per-account conversation inbox; eunha has conversation_participants
-- (simpler join table). Add the full Mastodon structure for API compatibility.
CREATE TABLE IF NOT EXISTS account_conversations (
    id                      BIGSERIAL PRIMARY KEY,
    account_id              BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    conversation_id         BIGINT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    participant_account_ids BIGINT[] NOT NULL DEFAULT '{}',
    status_ids              BIGINT[] NOT NULL DEFAULT '{}',
    last_status_id          BIGINT,
    lock_version            INTEGER NOT NULL DEFAULT 0,
    unread                  BOOLEAN NOT NULL DEFAULT false,
    UNIQUE (account_id, conversation_id, participant_account_ids)
);
CREATE INDEX IF NOT EXISTS index_account_conversations_on_conversation_id
    ON account_conversations(conversation_id);

-- ── account_deletion_requests ────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS account_deletion_requests (
    id         BIGSERIAL PRIMARY KEY,
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_account_deletion_requests_on_account_id
    ON account_deletion_requests(account_id);

-- ── account_migrations ───────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS account_migrations (
    id                BIGSERIAL PRIMARY KEY,
    account_id        BIGINT REFERENCES accounts(id) ON DELETE CASCADE,
    acct              TEXT NOT NULL DEFAULT '',
    followers_count   BIGINT NOT NULL DEFAULT 0,
    target_account_id BIGINT REFERENCES accounts(id) ON DELETE SET NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_account_migrations_on_account_id ON account_migrations(account_id);
CREATE INDEX IF NOT EXISTS index_account_migrations_on_target_account_id
    ON account_migrations(target_account_id) WHERE target_account_id IS NOT NULL;

-- ── account_relationship_severance_events ────────────────────────────────────
-- Junction between accounts and relationship_severance_events
CREATE TABLE IF NOT EXISTS account_relationship_severance_events (
    id                           BIGSERIAL PRIMARY KEY,
    account_id                   BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    relationship_severance_event_id BIGINT NOT NULL,
    followers_count              INTEGER NOT NULL DEFAULT 0,
    following_count              INTEGER NOT NULL DEFAULT 0,
    created_at                   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, relationship_severance_event_id)
);
CREATE INDEX IF NOT EXISTS index_account_relationship_severance_events_on_account_id
    ON account_relationship_severance_events(account_id);

-- ── relationship_severance_events ────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS relationship_severance_events (
    id            BIGSERIAL PRIMARY KEY,
    type          INTEGER NOT NULL DEFAULT 0,
    purged        BOOLEAN NOT NULL DEFAULT false,
    target_name   TEXT NOT NULL DEFAULT '',
    relationships_count INTEGER,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── severed_relationships ────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS severed_relationships (
    id                               BIGSERIAL PRIMARY KEY,
    relationship_severance_event_id  BIGINT NOT NULL REFERENCES relationship_severance_events(id) ON DELETE CASCADE,
    local_account_id                 BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    remote_account_id                BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    direction                        INTEGER NOT NULL DEFAULT 0,
    show_reblogs                     BOOLEAN,
    notify                           BOOLEAN,
    languages                        TEXT[],
    created_at                       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (relationship_severance_event_id, local_account_id, direction, remote_account_id)
);
CREATE INDEX IF NOT EXISTS index_severed_relationships_on_local_account_and_event
    ON severed_relationships(local_account_id, relationship_severance_event_id);
CREATE INDEX IF NOT EXISTS index_severed_relationships_on_remote_account_id
    ON severed_relationships(remote_account_id);

-- Add FK for account_relationship_severance_events now that relationship_severance_events exists
ALTER TABLE account_relationship_severance_events
    ADD CONSTRAINT account_relationship_severance_events_event_id_fkey
    FOREIGN KEY (relationship_severance_event_id)
    REFERENCES relationship_severance_events(id) ON DELETE CASCADE;

-- ── account_statuses_cleanup_policies ────────────────────────────────────────
CREATE TABLE IF NOT EXISTS account_statuses_cleanup_policies (
    id                  BIGSERIAL PRIMARY KEY,
    account_id          BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    enabled             BOOLEAN NOT NULL DEFAULT true,
    min_status_age      INTEGER NOT NULL DEFAULT 1209600,
    keep_direct         BOOLEAN NOT NULL DEFAULT true,
    keep_pinned         BOOLEAN NOT NULL DEFAULT true,
    keep_polls          BOOLEAN NOT NULL DEFAULT false,
    keep_media          BOOLEAN NOT NULL DEFAULT false,
    keep_self_fav       BOOLEAN NOT NULL DEFAULT true,
    keep_self_bookmark  BOOLEAN NOT NULL DEFAULT true,
    min_favs            INTEGER,
    min_reblogs         INTEGER,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_account_statuses_cleanup_policies_on_account_id
    ON account_statuses_cleanup_policies(account_id);

-- ── account_warning_presets ──────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS account_warning_presets (
    id         BIGSERIAL PRIMARY KEY,
    text       TEXT NOT NULL DEFAULT '',
    title      TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── accounts_tags ─────────────────────────────────────────────────────────────
-- Account ↔ tag associations (for account tag suggestions / indexing).
-- Note: tags.id is UUID in eunha (BIGINT in Mastodon); FK type matches eunha.
CREATE TABLE IF NOT EXISTS accounts_tags (
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    tag_id     UUID   NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, tag_id)
);
CREATE INDEX IF NOT EXISTS index_accounts_tags_on_tag_id ON accounts_tags(tag_id);

-- ── appeals ──────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS appeals (
    id                     BIGSERIAL PRIMARY KEY,
    account_id             BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    account_warning_id     BIGINT NOT NULL UNIQUE REFERENCES account_warnings(id) ON DELETE CASCADE,
    text                   TEXT NOT NULL DEFAULT '',
    approved_at            TIMESTAMPTZ,
    approved_by_account_id BIGINT REFERENCES accounts(id) ON DELETE SET NULL,
    rejected_at            TIMESTAMPTZ,
    rejected_by_account_id BIGINT REFERENCES accounts(id) ON DELETE SET NULL,
    created_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_appeals_on_account_id ON appeals(account_id);
CREATE INDEX IF NOT EXISTS index_appeals_on_approved_by_account_id
    ON appeals(approved_by_account_id) WHERE approved_by_account_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS index_appeals_on_rejected_by_account_id
    ON appeals(rejected_by_account_id) WHERE rejected_by_account_id IS NOT NULL;

-- ── backups ───────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS backups (
    id           BIGSERIAL PRIMARY KEY,
    user_id      UUID REFERENCES users(id) ON DELETE SET NULL,
    dump_file    TEXT,
    processed    BOOLEAN NOT NULL DEFAULT false,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── bulk_imports ─────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS bulk_imports (
    id                BIGSERIAL PRIMARY KEY,
    account_id        BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    type              INTEGER NOT NULL DEFAULT 0,
    state             INTEGER NOT NULL DEFAULT 0,
    total_items       INTEGER NOT NULL DEFAULT 0,
    imported_items    INTEGER NOT NULL DEFAULT 0,
    processed_items   INTEGER NOT NULL DEFAULT 0,
    finished_at       TIMESTAMPTZ,
    overwrite         BOOLEAN NOT NULL DEFAULT false,
    likely_mismatched BOOLEAN NOT NULL DEFAULT false,
    original_filename TEXT NOT NULL DEFAULT '',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_bulk_imports_on_account_id ON bulk_imports(account_id);

CREATE TABLE IF NOT EXISTS bulk_import_rows (
    id             BIGSERIAL PRIMARY KEY,
    bulk_import_id BIGINT NOT NULL REFERENCES bulk_imports(id) ON DELETE CASCADE,
    data           JSONB,
    account_id     BIGINT REFERENCES accounts(id) ON DELETE SET NULL,
    state          INTEGER NOT NULL DEFAULT 0,
    original_line  INTEGER,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_bulk_import_rows_on_bulk_import_id ON bulk_import_rows(bulk_import_id);

-- ── custom_emoji_categories ───────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS custom_emoji_categories (
    id         BIGSERIAL PRIMARY KEY,
    name       TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE custom_emojis
    ADD COLUMN IF NOT EXISTS category_id BIGINT REFERENCES custom_emoji_categories(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS uri         TEXT,
    ADD COLUMN IF NOT EXISTS updated_at  TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── identities ───────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS identities (
    id         BIGSERIAL PRIMARY KEY,
    user_id    UUID REFERENCES users(id) ON DELETE CASCADE,
    provider   TEXT NOT NULL DEFAULT '',
    uid        TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_identities_on_user_id ON identities(user_id);

-- ── instance_moderation_notes ────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS instance_moderation_notes (
    id         BIGSERIAL PRIMARY KEY,
    content    TEXT NOT NULL DEFAULT '',
    domain     TEXT NOT NULL DEFAULT '',
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_instance_moderation_notes_on_domain ON instance_moderation_notes(domain);

-- ── login_activities ─────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS login_activities (
    id                    BIGSERIAL PRIMARY KEY,
    user_id               UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    authentication_method TEXT,
    provider              TEXT,
    success               BOOLEAN,
    failure_reason        TEXT,
    ip                    INET,
    user_agent            TEXT,
    created_at            TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS index_login_activities_on_user_id ON login_activities(user_id);

-- ── notification_permissions ─────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS notification_permissions (
    id               BIGSERIAL PRIMARY KEY,
    account_id       BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    from_account_id  BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_notification_permissions_on_account_id ON notification_permissions(account_id);
CREATE INDEX IF NOT EXISTS index_notification_permissions_on_from_account_id ON notification_permissions(from_account_id);

-- ── preview_card_providers ───────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS preview_card_providers (
    id              BIGSERIAL PRIMARY KEY,
    domain          TEXT NOT NULL DEFAULT '',
    trendable       BOOLEAN,
    reviewed_at     TIMESTAMPTZ,
    requested_review_at TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── preview_card_trends ───────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS preview_card_trends (
    id              BIGSERIAL PRIMARY KEY,
    preview_card_id BIGINT NOT NULL UNIQUE REFERENCES preview_cards(id) ON DELETE CASCADE,
    score           DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    rank            INTEGER NOT NULL DEFAULT 0,
    allowed         BOOLEAN NOT NULL DEFAULT false,
    language        TEXT
);

-- ── relays ────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS relays (
    id                 BIGSERIAL PRIMARY KEY,
    inbox_url          TEXT NOT NULL DEFAULT '',
    follow_activity_id TEXT,
    state              INTEGER NOT NULL DEFAULT 0,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── rule_translations ─────────────────────────────────────────────────────────
-- Note: eunha stores rules as JSONB in instances; mastodon has a rules table.
-- Create rules + rule_translations for API compatibility.
CREATE TABLE IF NOT EXISTS rules (
    id         BIGSERIAL PRIMARY KEY,
    priority   INTEGER NOT NULL DEFAULT 0,
    deleted_at TIMESTAMPTZ,
    text       TEXT NOT NULL DEFAULT '',
    hint       TEXT NOT NULL DEFAULT '',
    instance_id UUID REFERENCES instances(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS rule_translations (
    id         BIGSERIAL PRIMARY KEY,
    rule_id    BIGINT NOT NULL REFERENCES rules(id) ON DELETE CASCADE,
    locale     TEXT NOT NULL DEFAULT '',
    text       TEXT NOT NULL DEFAULT '',
    hint       TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_rule_translations_on_rule_id ON rule_translations(rule_id);

-- ── session_activations ──────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS session_activations (
    id            BIGSERIAL PRIMARY KEY,
    session_id    TEXT NOT NULL UNIQUE,
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_agent    TEXT NOT NULL DEFAULT '',
    ip            INET,
    access_token_id UUID REFERENCES oauth_access_tokens(id) ON DELETE SET NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_session_activations_on_user_id ON session_activations(user_id);
CREATE INDEX IF NOT EXISTS index_session_activations_on_access_token_id
    ON session_activations(access_token_id) WHERE access_token_id IS NOT NULL;

-- ── settings ─────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS settings (
    id           BIGSERIAL PRIMARY KEY,
    var          TEXT NOT NULL,
    value        TEXT,
    thing_type   TEXT,
    thing_id     BIGINT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (thing_type, thing_id, var)
);

-- ── site_uploads ─────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS site_uploads (
    id           BIGSERIAL PRIMARY KEY,
    var          TEXT NOT NULL DEFAULT '',
    file_url     TEXT,
    meta         JSONB,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── software_updates ─────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS software_updates (
    id            BIGSERIAL PRIMARY KEY,
    version       TEXT NOT NULL DEFAULT '',
    urgent        BOOLEAN NOT NULL DEFAULT false,
    type          INTEGER NOT NULL DEFAULT 0,
    release_notes TEXT NOT NULL DEFAULT '',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── status_trends ─────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS status_trends (
    id         BIGSERIAL PRIMARY KEY,
    status_id  BIGINT NOT NULL UNIQUE REFERENCES statuses(id) ON DELETE CASCADE,
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    score      DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    rank       INTEGER NOT NULL DEFAULT 0,
    allowed    BOOLEAN NOT NULL DEFAULT false,
    language   TEXT
);
CREATE INDEX IF NOT EXISTS index_status_trends_on_account_id ON status_trends(account_id);

-- ── tag_trends ────────────────────────────────────────────────────────────────
-- Note: tags.id is UUID in eunha (BIGINT in Mastodon); FK type matches eunha.
CREATE TABLE IF NOT EXISTS tag_trends (
    id       BIGSERIAL PRIMARY KEY,
    tag_id   UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    score    DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    rank     INTEGER NOT NULL DEFAULT 0,
    allowed  BOOLEAN NOT NULL DEFAULT false,
    language TEXT NOT NULL DEFAULT '',
    UNIQUE (tag_id, language)
);

-- ── terms_of_services ────────────────────────────────────────────────────────
-- Mastodon has a terms_of_services table; eunha stores ToS text in instances.
-- Add the table for API compatibility; populate from instances.
CREATE TABLE IF NOT EXISTS terms_of_services (
    id           BIGSERIAL PRIMARY KEY,
    text         TEXT NOT NULL DEFAULT '',
    changelog    TEXT NOT NULL DEFAULT '',
    published_at TIMESTAMPTZ,
    notification_sent_at TIMESTAMPTZ,
    instance_id  UUID REFERENCES instances(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_terms_of_services_on_published_at
    ON terms_of_services(published_at) WHERE published_at IS NOT NULL;

INSERT INTO terms_of_services (text, instance_id, published_at)
SELECT terms_of_service, id, now()
FROM instances
WHERE terms_of_service != ''
ON CONFLICT DO NOTHING;

-- ── tombstones ───────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS tombstones (
    id           BIGSERIAL PRIMARY KEY,
    account_id   BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    uri          TEXT NOT NULL,
    by_moderator BOOLEAN,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_tombstones_on_account_id ON tombstones(account_id);
CREATE INDEX IF NOT EXISTS index_tombstones_on_uri ON tombstones(uri);

-- ── unavailable_domains ───────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS unavailable_domains (
    id         BIGSERIAL PRIMARY KEY,
    domain     TEXT NOT NULL UNIQUE DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── user_invite_requests ─────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS user_invite_requests (
    id         BIGSERIAL PRIMARY KEY,
    user_id    UUID REFERENCES users(id) ON DELETE CASCADE,
    text       TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_user_invite_requests_on_user_id ON user_invite_requests(user_id);

-- ── username_blocks ───────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS username_blocks (
    id         BIGSERIAL PRIMARY KEY,
    username   TEXT NOT NULL,
    exact_match BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── web_settings ─────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS web_settings (
    id         BIGSERIAL PRIMARY KEY,
    user_id    UUID NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    data       JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── webauthn_credentials ─────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS webauthn_credentials (
    id          BIGSERIAL PRIMARY KEY,
    external_id TEXT NOT NULL UNIQUE,
    public_key  TEXT NOT NULL,
    nickname    TEXT NOT NULL,
    sign_count  BIGINT NOT NULL DEFAULT 0,
    user_id     UUID REFERENCES users(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS index_webauthn_credentials_on_user_id_and_nickname
    ON webauthn_credentials(user_id, nickname);

-- ── webhooks ─────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS webhooks (
    id         BIGSERIAL PRIMARY KEY,
    url        TEXT NOT NULL UNIQUE DEFAULT '',
    events     TEXT[] NOT NULL DEFAULT '{}',
    secret     TEXT NOT NULL DEFAULT '',
    enabled    BOOLEAN NOT NULL DEFAULT true,
    template   TEXT,
    instance_id UUID REFERENCES instances(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── account_warning_presets (already created above) ──────────────────────────

-- ── FASP tables ───────────────────────────────────────────────────────────────
-- Fediverse Auxiliary Service Protocol tables.
CREATE TABLE IF NOT EXISTS fasp_providers (
    id                   BIGSERIAL PRIMARY KEY,
    name                 TEXT NOT NULL DEFAULT '',
    base_url             TEXT NOT NULL DEFAULT '',
    sign_in_url          TEXT,
    remote_identifier    TEXT,
    provider_public_key_base64 TEXT NOT NULL DEFAULT '',
    server_private_key_base64  TEXT NOT NULL DEFAULT '',
    server_public_key_base64   TEXT NOT NULL DEFAULT '',
    confirmed            BOOLEAN NOT NULL DEFAULT false,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS fasp_subscriptions (
    id              BIGSERIAL PRIMARY KEY,
    fasp_provider_id BIGINT NOT NULL REFERENCES fasp_providers(id) ON DELETE CASCADE,
    category        INTEGER NOT NULL DEFAULT 0,
    active          BOOLEAN NOT NULL DEFAULT true,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_fasp_subscriptions_on_fasp_provider_id
    ON fasp_subscriptions(fasp_provider_id);

CREATE TABLE IF NOT EXISTS fasp_backfill_requests (
    id               BIGSERIAL PRIMARY KEY,
    fasp_provider_id BIGINT NOT NULL REFERENCES fasp_providers(id) ON DELETE CASCADE,
    max_count        INTEGER NOT NULL DEFAULT 0,
    fulfilled        BOOLEAN NOT NULL DEFAULT false,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS fasp_debug_callbacks (
    id               BIGSERIAL PRIMARY KEY,
    fasp_provider_id BIGINT NOT NULL REFERENCES fasp_providers(id) ON DELETE CASCADE,
    payload          TEXT NOT NULL DEFAULT '',
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS fasp_follow_recommendations (
    id               BIGSERIAL PRIMARY KEY,
    fasp_provider_id BIGINT NOT NULL REFERENCES fasp_providers(id) ON DELETE CASCADE,
    account_id       BIGINT REFERENCES accounts(id) ON DELETE CASCADE,
    acct             TEXT NOT NULL DEFAULT '',
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
