-- Align web_push_subscriptions with Mastodon schema:
-- remove old p256dh/auth column aliases (superseded by key_p256dh/key_auth added in 067),
-- consolidate alert booleans + policy into the JSON data column, and make
-- media_attachments.account_id nullable to match Mastodon.

-- Drop superseded aliases added in migration 021 (key_* added in 067 took over)
ALTER TABLE web_push_subscriptions
    DROP COLUMN IF EXISTS p256dh,
    DROP COLUMN IF EXISTS auth;

-- Backfill data JSON from alert columns (only present if migration 021 added them)
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'web_push_subscriptions' AND column_name = 'alert_follow'
    ) THEN
        UPDATE web_push_subscriptions SET data = jsonb_build_object(
            'alerts', jsonb_build_object(
                'follow',    alert_follow,
                'favourite', alert_favourite,
                'reblog',    alert_reblog,
                'mention',   alert_mention,
                'poll',      alert_poll,
                'status',    alert_status
            ),
            'policy', policy
        );
    END IF;
END $$;

-- Drop individual alert/policy columns (IF EXISTS makes this idempotent)
ALTER TABLE web_push_subscriptions
    DROP COLUMN IF EXISTS alert_follow,
    DROP COLUMN IF EXISTS alert_favourite,
    DROP COLUMN IF EXISTS alert_reblog,
    DROP COLUMN IF EXISTS alert_mention,
    DROP COLUMN IF EXISTS alert_poll,
    DROP COLUMN IF EXISTS alert_status,
    DROP COLUMN IF EXISTS policy;

-- Ensure data is non-null
UPDATE web_push_subscriptions SET data = '{}' WHERE data IS NULL;
ALTER TABLE web_push_subscriptions ALTER COLUMN data SET NOT NULL;
ALTER TABLE web_push_subscriptions ALTER COLUMN data SET DEFAULT '{}';

-- access_token_id: enforce NOT NULL (may already be set)
ALTER TABLE web_push_subscriptions ALTER COLUMN access_token_id SET NOT NULL;

-- media_attachments.account_id: nullable to match Mastodon (remote media has no local account)
ALTER TABLE media_attachments ALTER COLUMN account_id DROP NOT NULL;
