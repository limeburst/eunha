CREATE TABLE announcements (
    id          BIGSERIAL PRIMARY KEY,
    instance_id UUID NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    text        TEXT NOT NULL DEFAULT '',
    published   BOOLEAN NOT NULL DEFAULT true,
    all_day     BOOLEAN NOT NULL DEFAULT false,
    starts_at   TIMESTAMPTZ,
    ends_at     TIMESTAMPTZ,
    published_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE announcement_dismissals (
    announcement_id BIGINT NOT NULL REFERENCES announcements(id) ON DELETE CASCADE,
    account_id      UUID   NOT NULL REFERENCES accounts(id)      ON DELETE CASCADE,
    PRIMARY KEY (announcement_id, account_id)
);
