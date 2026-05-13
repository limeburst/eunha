-- Conversations for direct messages
CREATE TABLE conversations (
    id BIGSERIAL PRIMARY KEY,
    instance_id UUID NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE conversation_participants (
    conversation_id BIGINT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    account_id UUID NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    unread BOOLEAN NOT NULL DEFAULT false,
    PRIMARY KEY (conversation_id, account_id)
);

ALTER TABLE statuses ADD COLUMN conversation_id BIGINT REFERENCES conversations(id);
CREATE INDEX idx_statuses_conversation_id ON statuses(conversation_id) WHERE conversation_id IS NOT NULL;

-- Backfill: create a conversation for each existing direct-message thread root
DO $$
DECLARE
    r RECORD;
    conv_id BIGINT;
    rows_updated INT;
BEGIN
    FOR r IN
        SELECT s.id, s.instance_id, s.account_id
        FROM statuses s
        WHERE s.visibility = 'direct'
          AND s.deleted_at IS NULL
          AND (
              s.in_reply_to_id IS NULL
              OR NOT EXISTS (
                  SELECT 1 FROM statuses p
                  WHERE p.id = s.in_reply_to_id AND p.visibility = 'direct' AND p.deleted_at IS NULL
              )
          )
        ORDER BY s.id
    LOOP
        INSERT INTO conversations (instance_id, created_at, updated_at)
        VALUES (r.instance_id, now(), now())
        RETURNING id INTO conv_id;

        UPDATE statuses SET conversation_id = conv_id WHERE id = r.id;

        INSERT INTO conversation_participants (conversation_id, account_id, unread)
        VALUES (conv_id, r.account_id, false)
        ON CONFLICT DO NOTHING;

        INSERT INTO conversation_participants (conversation_id, account_id, unread)
        SELECT conv_id, m.account_id, true
        FROM mentions m
        WHERE m.status_id = r.id
        ON CONFLICT DO NOTHING;
    END LOOP;

    -- Propagate conversation_id down into direct replies (up to 10 levels)
    LOOP
        WITH updated AS (
            UPDATE statuses s
            SET conversation_id = parent.conversation_id
            FROM statuses parent
            WHERE s.in_reply_to_id = parent.id
              AND s.visibility = 'direct'
              AND s.deleted_at IS NULL
              AND s.conversation_id IS NULL
              AND parent.conversation_id IS NOT NULL
            RETURNING s.id
        )
        SELECT count(*) INTO rows_updated FROM updated;
        EXIT WHEN rows_updated = 0;
    END LOOP;
END;
$$;
