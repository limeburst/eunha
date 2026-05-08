-- Hosted instances (one per domain)
CREATE TABLE IF NOT EXISTS instances (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    domain      TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL DEFAULT '',
    description TEXT NOT NULL DEFAULT '',
    short_description TEXT NOT NULL DEFAULT '',
    contact_email TEXT,
    registrations_open BOOLEAN NOT NULL DEFAULT true,
    approval_required  BOOLEAN NOT NULL DEFAULT false,
    -- ActivePub actor keypair for instance-level signing (e.g. instance actor)
    private_key TEXT NOT NULL DEFAULT '',
    public_key  TEXT NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Accounts: local (domain IS NULL) and remote
CREATE TABLE IF NOT EXISTS accounts (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id     UUID NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    username        TEXT NOT NULL,
    -- NULL for local accounts; set for remote accounts
    domain          TEXT,
    display_name    TEXT NOT NULL DEFAULT '',
    note            TEXT NOT NULL DEFAULT '',
    note_text       TEXT NOT NULL DEFAULT '',  -- plain-text version
    url             TEXT NOT NULL DEFAULT '',
    uri             TEXT NOT NULL DEFAULT '',  -- ActivityPub actor URI
    avatar          TEXT,
    avatar_static   TEXT,
    header          TEXT,
    header_static   TEXT,
    private_key     TEXT,       -- only for local accounts
    public_key      TEXT NOT NULL DEFAULT '',
    followers_count BIGINT NOT NULL DEFAULT 0,
    following_count BIGINT NOT NULL DEFAULT 0,
    statuses_count  BIGINT NOT NULL DEFAULT 0,
    locked          BOOLEAN NOT NULL DEFAULT false,
    bot             BOOLEAN NOT NULL DEFAULT false,
    discoverable    BOOLEAN NOT NULL DEFAULT true,
    indexable       BOOLEAN NOT NULL DEFAULT false,
    moved_to_uri    TEXT,
    -- Remote inbox/outbox URLs
    inbox_url       TEXT NOT NULL DEFAULT '',
    outbox_url      TEXT NOT NULL DEFAULT '',
    shared_inbox_url TEXT,
    -- Suspend/silence
    suspended_at    TIMESTAMPTZ,
    silenced_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- username is unique per-instance for local accounts,
    -- and unique per (username, domain) globally
    CONSTRAINT accounts_local_unique  UNIQUE NULLS NOT DISTINCT (instance_id, username, domain)
);

CREATE UNIQUE INDEX IF NOT EXISTS accounts_uri_unique ON accounts(uri) WHERE uri != '';
CREATE INDEX IF NOT EXISTS accounts_by_instance ON accounts(instance_id);
CREATE INDEX IF NOT EXISTS accounts_by_domain    ON accounts(domain) WHERE domain IS NOT NULL;

-- Local user authentication (one per local account)
CREATE TABLE IF NOT EXISTS users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id      UUID NOT NULL UNIQUE REFERENCES accounts(id) ON DELETE CASCADE,
    email           TEXT NOT NULL,
    email_normalized TEXT NOT NULL,  -- lowercased, for unique constraint
    password_hash   TEXT NOT NULL,
    confirmed_at    TIMESTAMPTZ,
    -- email must be unique per instance
    instance_id     UUID NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (instance_id, email_normalized)
);

-- Snowflake ID sequence for statuses / notifications
CREATE SEQUENCE IF NOT EXISTS status_id_seq START 1;

CREATE TABLE IF NOT EXISTS statuses (
    id              BIGINT PRIMARY KEY DEFAULT nextval('status_id_seq'),
    instance_id     UUID NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    account_id      UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Raw Markdown/plain text source
    text            TEXT NOT NULL DEFAULT '',
    -- HTML-rendered content
    content         TEXT NOT NULL DEFAULT '',
    spoiler_text    TEXT NOT NULL DEFAULT '',
    in_reply_to_id          BIGINT REFERENCES statuses(id) ON DELETE SET NULL,
    in_reply_to_account_id  UUID REFERENCES accounts(id) ON DELETE SET NULL,
    reblog_of_id    BIGINT REFERENCES statuses(id) ON DELETE CASCADE,
    visibility      TEXT NOT NULL DEFAULT 'public'
                        CHECK (visibility IN ('public','unlisted','private','direct')),
    language        TEXT,
    sensitive       BOOLEAN NOT NULL DEFAULT false,
    -- For remote statuses
    url             TEXT,
    uri             TEXT UNIQUE,
    -- Counts (denormalized for performance)
    replies_count   BIGINT NOT NULL DEFAULT 0,
    reblogs_count   BIGINT NOT NULL DEFAULT 0,
    favourites_count BIGINT NOT NULL DEFAULT 0,
    -- Soft delete
    deleted_at      TIMESTAMPTZ,
    edited_at       TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS statuses_by_account    ON statuses(account_id, id DESC) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS statuses_by_instance   ON statuses(instance_id, id DESC) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS statuses_public        ON statuses(id DESC)
    WHERE visibility = 'public' AND deleted_at IS NULL AND reblog_of_id IS NULL;

-- Status edits (history)
CREATE TABLE IF NOT EXISTS status_edits (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    status_id       BIGINT NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    text            TEXT NOT NULL DEFAULT '',
    content         TEXT NOT NULL DEFAULT '',
    spoiler_text    TEXT NOT NULL DEFAULT '',
    sensitive       BOOLEAN NOT NULL DEFAULT false,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Media attachments
CREATE SEQUENCE IF NOT EXISTS media_id_seq START 1;

CREATE TABLE IF NOT EXISTS media_attachments (
    id              BIGINT PRIMARY KEY DEFAULT nextval('media_id_seq'),
    account_id      UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    status_id       BIGINT REFERENCES statuses(id) ON DELETE SET NULL,
    media_type      TEXT NOT NULL DEFAULT 'unknown'
                        CHECK (media_type IN ('image','video','gifv','audio','unknown')),
    -- Storage path / URL
    file_key        TEXT,
    file_url        TEXT,
    preview_key     TEXT,
    preview_url     TEXT,
    -- Remote URL for federated media
    remote_url      TEXT,
    description     TEXT,
    blurhash        TEXT,
    -- JSON: {width, height, size, aspect, original, small}
    meta            JSONB,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS media_by_account ON media_attachments(account_id);
CREATE INDEX IF NOT EXISTS media_by_status  ON media_attachments(status_id) WHERE status_id IS NOT NULL;

-- Polls
CREATE TABLE IF NOT EXISTS polls (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    status_id   BIGINT NOT NULL UNIQUE REFERENCES statuses(id) ON DELETE CASCADE,
    account_id  UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    options     JSONB NOT NULL DEFAULT '[]',  -- [{title, votes_count}]
    votes_count BIGINT NOT NULL DEFAULT 0,
    voters_count BIGINT,
    multiple    BOOLEAN NOT NULL DEFAULT false,
    expires_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Poll votes
CREATE TABLE IF NOT EXISTS poll_votes (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    poll_id     UUID NOT NULL REFERENCES polls(id) ON DELETE CASCADE,
    account_id  UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    choice      INT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (poll_id, account_id, choice)
);

-- Follow relationships
CREATE TABLE IF NOT EXISTS follows (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id          UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    target_account_id   UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    state               TEXT NOT NULL DEFAULT 'accepted'
                            CHECK (state IN ('pending', 'accepted')),
    uri                 TEXT UNIQUE,  -- ActivityPub activity URI
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, target_account_id)
);

CREATE INDEX IF NOT EXISTS follows_by_target ON follows(target_account_id) WHERE state = 'accepted';

-- Blocks
CREATE TABLE IF NOT EXISTS blocks (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id          UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    target_account_id   UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, target_account_id)
);

-- Mutes
CREATE TABLE IF NOT EXISTS mutes (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id          UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    target_account_id   UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    hide_notifications  BOOLEAN NOT NULL DEFAULT true,
    expires_at          TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, target_account_id)
);

-- Favourites (likes)
CREATE TABLE IF NOT EXISTS favourites (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id  UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    status_id   BIGINT NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    uri         TEXT UNIQUE,  -- ActivityPub activity URI
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, status_id)
);

-- Bookmarks
CREATE TABLE IF NOT EXISTS bookmarks (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id  UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    status_id   BIGINT NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, status_id)
);

-- Notifications
CREATE SEQUENCE IF NOT EXISTS notification_id_seq START 1;

CREATE TABLE IF NOT EXISTS notifications (
    id              BIGINT PRIMARY KEY DEFAULT nextval('notification_id_seq'),
    account_id      UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    from_account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    notification_type TEXT NOT NULL
        CHECK (notification_type IN (
            'mention','reblog','favourite','follow','follow_request',
            'poll','update','admin.sign_up','admin.report',
            'severed_relationships','moderation_warning'
        )),
    status_id       BIGINT REFERENCES statuses(id) ON DELETE CASCADE,
    read            BOOLEAN NOT NULL DEFAULT false,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS notifications_by_account ON notifications(account_id, id DESC);

-- OAuth applications
CREATE TABLE IF NOT EXISTS oauth_applications (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id     UUID NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    client_id       TEXT NOT NULL UNIQUE,
    client_secret   TEXT NOT NULL,
    redirect_uris   TEXT NOT NULL DEFAULT 'urn:ietf:wg:oauth:2.0:oob',
    scopes          TEXT NOT NULL DEFAULT 'read',
    website         TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- OAuth authorization codes
CREATE TABLE IF NOT EXISTS oauth_authorization_codes (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    application_id  UUID NOT NULL REFERENCES oauth_applications(id) ON DELETE CASCADE,
    account_id      UUID REFERENCES accounts(id) ON DELETE CASCADE,
    code            TEXT NOT NULL UNIQUE,
    redirect_uri    TEXT NOT NULL,
    scopes          TEXT NOT NULL DEFAULT 'read',
    code_challenge  TEXT,
    code_challenge_method TEXT,
    expires_at      TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- OAuth access tokens
CREATE TABLE IF NOT EXISTS oauth_access_tokens (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    application_id  UUID REFERENCES oauth_applications(id) ON DELETE CASCADE,
    account_id      UUID REFERENCES accounts(id) ON DELETE CASCADE,
    token           TEXT NOT NULL UNIQUE,
    refresh_token   TEXT UNIQUE,
    scopes          TEXT NOT NULL DEFAULT 'read',
    expires_at      TIMESTAMPTZ,
    revoked_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS tokens_by_account ON oauth_access_tokens(account_id) WHERE revoked_at IS NULL;

-- Federation: outgoing activity queue
CREATE TABLE IF NOT EXISTS outbox_queue (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id     UUID NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    account_id      UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    activity        JSONB NOT NULL,
    inbox_url       TEXT NOT NULL,
    attempts        INT NOT NULL DEFAULT 0,
    last_attempt_at TIMESTAMPTZ,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    failed_at       TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS outbox_queue_pending ON outbox_queue(next_attempt_at)
    WHERE failed_at IS NULL;

-- Federation: known remote instances (for federation stats / blocking)
CREATE TABLE IF NOT EXISTS remote_instances (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    domain      TEXT NOT NULL UNIQUE,
    software    TEXT,
    version     TEXT,
    suspended   BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Hashtags
CREATE TABLE IF NOT EXISTS tags (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL UNIQUE,  -- lowercase, no #
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS status_tags (
    status_id   BIGINT NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    tag_id      UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (status_id, tag_id)
);

CREATE INDEX IF NOT EXISTS status_tags_by_tag ON status_tags(tag_id);

-- Mentions (extracted from status content)
CREATE TABLE IF NOT EXISTS mentions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    status_id   BIGINT NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    account_id  UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    UNIQUE (status_id, account_id)
);

-- Custom emoji
CREATE TABLE IF NOT EXISTS custom_emojis (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id     UUID NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    shortcode       TEXT NOT NULL,
    domain          TEXT,  -- NULL for local
    image_url       TEXT NOT NULL,
    static_image_url TEXT,
    visible_in_picker BOOLEAN NOT NULL DEFAULT true,
    disabled        BOOLEAN NOT NULL DEFAULT false,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (instance_id, shortcode)
);
