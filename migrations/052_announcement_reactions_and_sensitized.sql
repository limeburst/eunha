CREATE TABLE announcement_reactions (
    id              BIGSERIAL PRIMARY KEY,
    announcement_id BIGINT NOT NULL REFERENCES announcements(id) ON DELETE CASCADE,
    account_id      BIGINT NOT NULL REFERENCES accounts(id)      ON DELETE CASCADE,
    name            TEXT   NOT NULL,
    custom_emoji_id UUID   REFERENCES custom_emojis(id) ON DELETE CASCADE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (announcement_id, account_id, name)
);

ALTER TABLE accounts
    ADD COLUMN IF NOT EXISTS sensitized_at TIMESTAMPTZ;
