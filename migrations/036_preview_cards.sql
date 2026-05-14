CREATE TABLE IF NOT EXISTS preview_cards (
    id           BIGSERIAL PRIMARY KEY,
    url          TEXT        NOT NULL UNIQUE,
    title        TEXT        NOT NULL DEFAULT '',
    description  TEXT        NOT NULL DEFAULT '',
    card_type    TEXT        NOT NULL DEFAULT 'link',
    image_url    TEXT,
    author_name  TEXT        NOT NULL DEFAULT '',
    author_url   TEXT        NOT NULL DEFAULT '',
    provider_name TEXT       NOT NULL DEFAULT '',
    provider_url TEXT        NOT NULL DEFAULT '',
    html         TEXT        NOT NULL DEFAULT '',
    width        INT         NOT NULL DEFAULT 0,
    height       INT         NOT NULL DEFAULT 0,
    embed_url    TEXT        NOT NULL DEFAULT '',
    blurhash     TEXT,
    fetched_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS status_preview_cards (
    status_id BIGINT NOT NULL REFERENCES statuses(id) ON DELETE CASCADE,
    card_id   BIGINT NOT NULL REFERENCES preview_cards(id) ON DELETE CASCADE,
    PRIMARY KEY (status_id, card_id)
);
