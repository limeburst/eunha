-- Tables for data migrated from Mastodon that had no prior eunha equivalent.

CREATE TABLE status_pins (
    id          BIGSERIAL PRIMARY KEY,
    account_id  UUID   NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    status_id   BIGINT NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, status_id)
);

CREATE TABLE account_notes (
    id                BIGSERIAL PRIMARY KEY,
    account_id        UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    target_account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    comment           TEXT NOT NULL DEFAULT '',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, target_account_id)
);

CREATE TABLE lists (
    id             BIGSERIAL PRIMARY KEY,
    account_id     UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    title          TEXT NOT NULL DEFAULT '',
    replies_policy TEXT NOT NULL DEFAULT 'list',
    exclusive      BOOLEAN NOT NULL DEFAULT false,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE list_accounts (
    id         BIGSERIAL PRIMARY KEY,
    list_id    BIGINT NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    account_id UUID   NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    UNIQUE (list_id, account_id)
);

CREATE TABLE custom_filters (
    id         BIGSERIAL PRIMARY KEY,
    account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ,
    phrase     TEXT NOT NULL DEFAULT '',
    context    TEXT[] NOT NULL DEFAULT '{}',
    action     TEXT NOT NULL DEFAULT 'warn',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE custom_filter_keywords (
    id               BIGSERIAL PRIMARY KEY,
    custom_filter_id BIGINT NOT NULL REFERENCES custom_filters(id) ON DELETE CASCADE,
    keyword          TEXT NOT NULL DEFAULT '',
    whole_word       BOOLEAN NOT NULL DEFAULT true,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE custom_filter_statuses (
    id               BIGSERIAL PRIMARY KEY,
    custom_filter_id BIGINT NOT NULL REFERENCES custom_filters(id) ON DELETE CASCADE,
    status_id        BIGINT NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE featured_tags (
    id             BIGSERIAL PRIMARY KEY,
    account_id     UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    tag_id         UUID NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    name           TEXT,
    statuses_count BIGINT NOT NULL DEFAULT 0,
    last_status_at TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, tag_id)
);

CREATE TABLE domain_blocks (
    id              BIGSERIAL PRIMARY KEY,
    domain          TEXT NOT NULL UNIQUE,
    severity        TEXT NOT NULL DEFAULT 'silence',
    reject_media    BOOLEAN NOT NULL DEFAULT false,
    reject_reports  BOOLEAN NOT NULL DEFAULT false,
    private_comment TEXT,
    public_comment  TEXT,
    obfuscate       BOOLEAN NOT NULL DEFAULT false,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE domain_allows (
    id         BIGSERIAL PRIMARY KEY,
    domain     TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE reports (
    id                          BIGSERIAL PRIMARY KEY,
    account_id                  UUID   NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    target_account_id           UUID   NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    assigned_account_id         UUID   REFERENCES accounts(id) ON DELETE SET NULL,
    action_taken_by_account_id  UUID   REFERENCES accounts(id) ON DELETE SET NULL,
    status_ids                  BIGINT[] NOT NULL DEFAULT '{}',
    comment                     TEXT NOT NULL DEFAULT '',
    forwarded                   BOOLEAN,
    category                    TEXT NOT NULL DEFAULT 'other',
    action_taken_at             TIMESTAMPTZ,
    uri                         TEXT,
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE report_notes (
    id         BIGSERIAL PRIMARY KEY,
    content    TEXT NOT NULL,
    report_id  BIGINT NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
    account_id UUID   NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE account_warnings (
    id                BIGSERIAL PRIMARY KEY,
    account_id        UUID REFERENCES accounts(id) ON DELETE SET NULL,
    target_account_id UUID REFERENCES accounts(id) ON DELETE CASCADE,
    action            TEXT NOT NULL DEFAULT 'none',
    text              TEXT NOT NULL DEFAULT '',
    status_ids        BIGINT[],
    report_id         BIGINT REFERENCES reports(id) ON DELETE SET NULL,
    overruled_at      TIMESTAMPTZ,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE account_moderation_notes (
    id                BIGSERIAL PRIMARY KEY,
    content           TEXT NOT NULL,
    account_id        UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    target_account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE admin_action_logs (
    id               BIGSERIAL PRIMARY KEY,
    account_id       UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    action           TEXT NOT NULL DEFAULT '',
    target_type      TEXT,
    target_id        BIGINT,
    human_identifier TEXT,
    route_param      TEXT,
    permalink        TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE scheduled_statuses (
    id           BIGSERIAL PRIMARY KEY,
    account_id   UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    scheduled_at TIMESTAMPTZ,
    params       JSONB,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
