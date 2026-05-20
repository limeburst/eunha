-- Replace conversation_participants with account_conversations (Mastodon-compatible).
-- Drop dead tables: outbox_queue, remote_instances.
-- Written idempotently: all operations guarded with IF EXISTS or DO blocks.

-- ── Migrate conversation_participants → account_conversations ────────────────
DO $$
BEGIN
    IF EXISTS (SELECT FROM information_schema.tables WHERE table_schema = 'public' AND table_name = 'conversation_participants') THEN
        INSERT INTO account_conversations (
            account_id,
            conversation_id,
            participant_account_ids,
            status_ids,
            last_status_id,
            unread
        )
        SELECT
            cp.account_id,
            cp.conversation_id,
            COALESCE(
                ARRAY(
                    SELECT other.account_id
                    FROM conversation_participants other
                    WHERE other.conversation_id = cp.conversation_id
                      AND other.account_id != cp.account_id
                    ORDER BY other.account_id
                ),
                '{}'::bigint[]
            ),
            COALESCE(
                ARRAY(
                    SELECT s.id
                    FROM statuses s
                    WHERE s.conversation_id = cp.conversation_id
                      AND s.deleted_at IS NULL
                    ORDER BY s.id
                ),
                '{}'::bigint[]
            ),
            (
                SELECT max(s.id)
                FROM statuses s
                WHERE s.conversation_id = cp.conversation_id
                  AND s.deleted_at IS NULL
            ),
            cp.unread
        FROM conversation_participants cp
        ON CONFLICT (account_id, conversation_id, participant_account_ids) DO UPDATE
            SET unread         = EXCLUDED.unread,
                status_ids     = EXCLUDED.status_ids,
                last_status_id = EXCLUDED.last_status_id;

        DROP TABLE conversation_participants;
    END IF;
END $$;

-- ── Drop dead tables ─────────────────────────────────────────────────────────
DROP TABLE IF EXISTS outbox_queue;
DROP TABLE IF EXISTS remote_instances;
