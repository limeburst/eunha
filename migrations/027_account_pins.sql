CREATE TABLE account_pins (
    id                BIGSERIAL PRIMARY KEY,
    account_id        UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    target_account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, target_account_id)
);
