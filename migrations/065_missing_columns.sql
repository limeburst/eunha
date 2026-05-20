-- Add missing columns to tables that exist in both eunha and mastodon_src.

-- ── accounts ─────────────────────────────────────────────────────────────────
ALTER TABLE accounts
    ADD COLUMN IF NOT EXISTS actor_type       TEXT,
    ADD COLUMN IF NOT EXISTS also_known_as    TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS featured_collection_url TEXT,
    ADD COLUMN IF NOT EXISTS followers_url    TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS following_url    TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS last_webfingered_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS memorial         BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS moved_to_account_id BIGINT REFERENCES accounts(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS protocol         INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS requested_review_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS reviewed_at      TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS suspension_origin INTEGER,
    ADD COLUMN IF NOT EXISTS trendable        BOOLEAN,
    ADD COLUMN IF NOT EXISTS id_scheme        INTEGER;

-- ── statuses ──────────────────────────────────────────────────────────────────
ALTER TABLE statuses
    ADD COLUMN IF NOT EXISTS fetched_replies_at           TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS local                        BOOLEAN,
    ADD COLUMN IF NOT EXISTS ordered_media_attachment_ids BIGINT[],
    -- poll_id: Mastodon stores on status; eunha links via polls.status_id unique FK.
    -- Add for compatibility; populate via trigger or application logic.
    ADD COLUMN IF NOT EXISTS poll_id                      BIGINT,
    ADD COLUMN IF NOT EXISTS quote_approval_policy        INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS trendable                    BOOLEAN,
    ADD COLUMN IF NOT EXISTS updated_at                   TIMESTAMPTZ;

-- Populate local flag from whether account is local (no domain)
UPDATE statuses s SET local = (
    SELECT domain IS NULL FROM accounts a WHERE a.id = s.account_id
) WHERE local IS NULL;

-- ── notifications ─────────────────────────────────────────────────────────────
ALTER TABLE notifications
    ADD COLUMN IF NOT EXISTS filtered  BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS group_key TEXT;

CREATE INDEX IF NOT EXISTS index_notifications_on_account_id_and_group_key
    ON notifications(account_id, group_key) WHERE group_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS index_notifications_on_account_id_id_type
    ON notifications(account_id, id DESC, notification_type);
CREATE INDEX IF NOT EXISTS index_notifications_on_filtered
    ON notifications(account_id, id DESC, notification_type) WHERE filtered = false;

-- ── polls ─────────────────────────────────────────────────────────────────────
ALTER TABLE polls
    ADD COLUMN IF NOT EXISTS cached_tallies  BIGINT[],
    ADD COLUMN IF NOT EXISTS hide_totals     BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS last_fetched_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS lock_version    INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS updated_at      TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── preview_cards ─────────────────────────────────────────────────────────────
ALTER TABLE preview_cards
    ADD COLUMN IF NOT EXISTS author_account_id BIGINT REFERENCES accounts(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS language          TEXT,
    ADD COLUMN IF NOT EXISTS link_type         INTEGER,
    ADD COLUMN IF NOT EXISTS max_score         DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS max_score_at      TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS published_at      TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS trendable         BOOLEAN,
    ADD COLUMN IF NOT EXISTS "type"            INTEGER;

-- ── status_edits ─────────────────────────────────────────────────────────────
ALTER TABLE status_edits
    ADD COLUMN IF NOT EXISTS media_descriptions           TEXT[],
    ADD COLUMN IF NOT EXISTS ordered_media_attachment_ids BIGINT[],
    ADD COLUMN IF NOT EXISTS poll_options                 TEXT[],
    ADD COLUMN IF NOT EXISTS quote_id                     BIGINT REFERENCES quotes(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS updated_at                   TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── markers ───────────────────────────────────────────────────────────────────
ALTER TABLE markers
    ADD COLUMN IF NOT EXISTS created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS lock_version INTEGER NOT NULL DEFAULT 0;

-- ── blocks ────────────────────────────────────────────────────────────────────
ALTER TABLE blocks
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS uri        TEXT;

-- ── bookmarks ─────────────────────────────────────────────────────────────────
ALTER TABLE bookmarks
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── mutes ─────────────────────────────────────────────────────────────────────
ALTER TABLE mutes
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── account_pins ──────────────────────────────────────────────────────────────
ALTER TABLE account_pins
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── favourites ────────────────────────────────────────────────────────────────
ALTER TABLE favourites
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── follows ───────────────────────────────────────────────────────────────────
ALTER TABLE follows
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── mentions ──────────────────────────────────────────────────────────────────
ALTER TABLE mentions
    ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS silent     BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── poll_votes ────────────────────────────────────────────────────────────────
ALTER TABLE poll_votes
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS uri        TEXT;

-- ── announcement_reactions ────────────────────────────────────────────────────
ALTER TABLE announcement_reactions
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── tag_follows ───────────────────────────────────────────────────────────────
ALTER TABLE tag_follows
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── tags ──────────────────────────────────────────────────────────────────────
ALTER TABLE tags
    ADD COLUMN IF NOT EXISTS display_name        TEXT,
    ADD COLUMN IF NOT EXISTS last_status_at      TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS max_score           DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS max_score_at        TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS requested_review_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS reviewed_at         TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS trendable           BOOLEAN,
    ADD COLUMN IF NOT EXISTS listable            BOOLEAN;

-- ── account_aliases ───────────────────────────────────────────────────────────
ALTER TABLE account_aliases
    ADD COLUMN IF NOT EXISTS acct TEXT;

-- ── canonical_email_blocks ────────────────────────────────────────────────────
ALTER TABLE canonical_email_blocks
    ADD COLUMN IF NOT EXISTS reference_account_id BIGINT REFERENCES accounts(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS updated_at           TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── invites ───────────────────────────────────────────────────────────────────
ALTER TABLE invites
    ADD COLUMN IF NOT EXISTS autofollow BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS comment    TEXT,
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── reports ───────────────────────────────────────────────────────────────────
ALTER TABLE reports
    ADD COLUMN IF NOT EXISTS rule_ids      INTEGER[],
    ADD COLUMN IF NOT EXISTS application_id BIGINT REFERENCES oauth_applications(id) ON DELETE SET NULL;

-- ── list_accounts ─────────────────────────────────────────────────────────────
-- follow_id references follows.id (UUID in eunha)
-- follow_request_id references follow_requests.id (BIGINT, added in migration 066)
ALTER TABLE list_accounts
    ADD COLUMN IF NOT EXISTS follow_id UUID REFERENCES follows(id) ON DELETE SET NULL;

-- ── account_domain_blocks (renamed from user_domain_blocks) ──────────────────
-- unique constraint was added in 064; no further columns needed

-- ── oauth_applications ────────────────────────────────────────────────────────
-- Mastodon has: confidential, owner_id, owner_type, redirect_uri (singular), secret, superapp, uid
-- eunha has: client_id (≈uid), client_secret (≈secret), redirect_uris (plural), instance_id
-- Add the missing Mastodon-compatible columns as aliases/extras
ALTER TABLE oauth_applications
    ADD COLUMN IF NOT EXISTS confidential BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS superapp     BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS updated_at   TIMESTAMPTZ NOT NULL DEFAULT now();

-- ── notification_requests ────────────────────────────────────────────────────
-- 'dismissed' is extra in eunha and not in mastodon; keep it

-- ── scheduled_statuses ───────────────────────────────────────────────────────
-- 'created_at' is extra in eunha; keep it

-- ── Extract follow_requests from follows ────────────────────────────────────
-- Mastodon separates pending follows into a dedicated table.
-- eunha used follows.state = 'pending'; we now split them out.
CREATE TABLE IF NOT EXISTS follow_requests (
    id                BIGSERIAL PRIMARY KEY,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    account_id        BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    target_account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    show_reblogs      BOOLEAN NOT NULL DEFAULT true,
    uri               TEXT UNIQUE,
    notify            BOOLEAN NOT NULL DEFAULT false,
    languages         TEXT[] NOT NULL DEFAULT '{}',
    UNIQUE (account_id, target_account_id)
);
CREATE INDEX IF NOT EXISTS index_follow_requests_on_account_id_and_target_account_id
    ON follow_requests(account_id, target_account_id);

-- Migrate pending follows to follow_requests
INSERT INTO follow_requests
    (created_at, updated_at, account_id, target_account_id, show_reblogs, uri, notify, languages)
SELECT
    created_at,
    COALESCE(updated_at, created_at),
    account_id,
    target_account_id,
    show_reblogs,
    uri,
    notify,
    languages
FROM follows
WHERE state = 'pending'
ON CONFLICT DO NOTHING;

-- Remove migrated pending follows from the follows table
DELETE FROM follows WHERE state = 'pending';

-- Drop the state column (follows now only contains accepted follows)
ALTER TABLE follows DROP CONSTRAINT IF EXISTS follows_state_check;
ALTER TABLE follows DROP COLUMN IF EXISTS state;

-- follow_request_id FK added here since follow_requests now exists
ALTER TABLE list_accounts
    ADD COLUMN IF NOT EXISTS follow_request_id BIGINT REFERENCES follow_requests(id) ON DELETE SET NULL;

-- ── account_stats (denormalized → normalized) ─────────────────────────────────
-- Mastodon stores these in a separate table; eunha inlines them in accounts.
-- Create the table and populate from accounts; keep accounts columns in sync via app.
CREATE TABLE IF NOT EXISTS account_stats (
    id              BIGSERIAL PRIMARY KEY,
    account_id      BIGINT NOT NULL UNIQUE REFERENCES accounts(id) ON DELETE CASCADE,
    statuses_count  BIGINT NOT NULL DEFAULT 0,
    following_count BIGINT NOT NULL DEFAULT 0,
    followers_count BIGINT NOT NULL DEFAULT 0,
    last_status_at  TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_account_stats_on_account_id ON account_stats(account_id);
CREATE INDEX IF NOT EXISTS index_account_stats_on_last_status_at_and_account_id
    ON account_stats(last_status_at DESC NULLS LAST, account_id);

INSERT INTO account_stats (account_id, statuses_count, following_count, followers_count, last_status_at)
SELECT id, statuses_count, following_count, followers_count, last_status_at
FROM accounts
ON CONFLICT DO NOTHING;

-- ── status_stats (denormalized → normalized) ──────────────────────────────────
CREATE TABLE IF NOT EXISTS status_stats (
    id                        BIGSERIAL PRIMARY KEY,
    status_id                 BIGINT NOT NULL UNIQUE REFERENCES statuses(id) ON DELETE CASCADE,
    replies_count             BIGINT NOT NULL DEFAULT 0,
    reblogs_count             BIGINT NOT NULL DEFAULT 0,
    favourites_count          BIGINT NOT NULL DEFAULT 0,
    quotes_count              BIGINT NOT NULL DEFAULT 0,
    untrusted_favourites_count BIGINT,
    untrusted_reblogs_count   BIGINT,
    created_at                TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_status_stats_on_status_id ON status_stats(status_id);

INSERT INTO status_stats (status_id, replies_count, reblogs_count, favourites_count, quotes_count)
SELECT id, replies_count, reblogs_count, favourites_count, quotes_count
FROM statuses
WHERE deleted_at IS NULL
ON CONFLICT DO NOTHING;

-- ── notification_policies (denormalized → normalized) ──────────────────────────
-- Mastodon stores these per-account; eunha stored them per-user as booleans.
-- Map: notif_filter_not_following → for_not_following (0=accept,1=filter,2=drop)
--      eunha booleans default to false (= accept/0) or true (= filter/1)
CREATE TABLE IF NOT EXISTS notification_policies (
    id                  BIGSERIAL PRIMARY KEY,
    account_id          BIGINT NOT NULL UNIQUE REFERENCES accounts(id) ON DELETE CASCADE,
    for_not_following   INTEGER NOT NULL DEFAULT 0,
    for_not_followers   INTEGER NOT NULL DEFAULT 0,
    for_new_accounts    INTEGER NOT NULL DEFAULT 0,
    for_private_mentions INTEGER NOT NULL DEFAULT 1,
    for_limited_accounts INTEGER NOT NULL DEFAULT 1,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS index_notification_policies_on_account_id ON notification_policies(account_id);

INSERT INTO notification_policies
    (account_id, for_not_following, for_not_followers, for_new_accounts, for_private_mentions, for_limited_accounts)
SELECT
    a.id,
    CASE WHEN u.notif_filter_not_following     THEN 1 ELSE 0 END,
    CASE WHEN u.notif_filter_not_followers     THEN 1 ELSE 0 END,
    CASE WHEN u.notif_filter_new_accounts      THEN 1 ELSE 0 END,
    CASE WHEN u.notif_filter_private_mentions  THEN 1 ELSE 0 END,
    CASE WHEN u.notif_filter_limited_accounts  THEN 1 ELSE 0 END
FROM users u
JOIN accounts a ON a.id = u.account_id
ON CONFLICT DO NOTHING;

-- ── user_roles ────────────────────────────────────────────────────────────────
-- Mastodon stores roles in a table; eunha used a text column users.role.
-- Create the table with standard Mastodon roles and add role_id FK to users.
CREATE TABLE IF NOT EXISTS user_roles (
    id          BIGSERIAL PRIMARY KEY,
    name        TEXT NOT NULL DEFAULT '',
    color       TEXT NOT NULL DEFAULT '',
    position    INTEGER NOT NULL DEFAULT 0,
    permissions BIGINT NOT NULL DEFAULT 0,
    highlighted BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Standard Mastodon roles (id -99=owner, -1=moderator, 0=user implied by NULL role_id)
INSERT INTO user_roles (id, name, color, position, permissions, highlighted)
VALUES
    (-99, 'Owner',     '', 1000, 1048575, true),
    (-1,  'Admin',     '', 900,  458751,  true),
    (0,   'Moderator', '', 800,  14337,   true)
ON CONFLICT DO NOTHING;

ALTER TABLE users ADD COLUMN IF NOT EXISTS role_id BIGINT REFERENCES user_roles(id) ON DELETE SET NULL;

-- Populate role_id from existing text role column
UPDATE users SET role_id = -99 WHERE role = 'owner';
UPDATE users SET role_id = -1  WHERE role = 'admin';
UPDATE users SET role_id = 0   WHERE role = 'moderator';

CREATE INDEX IF NOT EXISTS index_users_on_role_id ON users(role_id) WHERE role_id IS NOT NULL;
