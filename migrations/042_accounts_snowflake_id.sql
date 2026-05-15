-- Migration 042: Change accounts.id from UUID to BIGINT (Snowflake)
-- All foreign keys referencing accounts.id are updated accordingly.
-- Existing accounts get Snowflake IDs derived from created_at;
-- future accounts are assigned IDs by the application (crate::snowflake).

BEGIN;

-- ── Step 1: Build UUID → Snowflake mapping ───────────────────────────────────
-- Lower 16 bits: row_number within same millisecond bucket (guarantees uniqueness).
CREATE TEMP TABLE _acct_map AS
SELECT
    id AS old_id,
    (
        ((EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT << 16)
        | ((ROW_NUMBER() OVER (
                PARTITION BY (EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT
                ORDER BY id
            ) - 1) & 65535)
    ) AS new_id
FROM accounts;

-- ── Step 2: Add BIGINT id_new to accounts ────────────────────────────────────
ALTER TABLE accounts ADD COLUMN id_new BIGINT;
UPDATE accounts SET id_new = (SELECT new_id FROM _acct_map WHERE old_id = id);

-- ── Step 3: Add BIGINT shadow columns to every referencing table ─────────────
ALTER TABLE account_aliases           ADD COLUMN account_id_new               BIGINT;
ALTER TABLE account_moderation_notes  ADD COLUMN account_id_new               BIGINT;
ALTER TABLE account_moderation_notes  ADD COLUMN target_account_id_new        BIGINT;
ALTER TABLE account_notes             ADD COLUMN account_id_new               BIGINT;
ALTER TABLE account_notes             ADD COLUMN target_account_id_new        BIGINT;
ALTER TABLE account_pins              ADD COLUMN account_id_new               BIGINT;
ALTER TABLE account_pins              ADD COLUMN target_account_id_new        BIGINT;
ALTER TABLE account_warnings          ADD COLUMN account_id_new               BIGINT;
ALTER TABLE account_warnings          ADD COLUMN target_account_id_new        BIGINT;
ALTER TABLE admin_action_logs         ADD COLUMN account_id_new               BIGINT;
ALTER TABLE announcement_dismissals   ADD COLUMN account_id_new               BIGINT;
ALTER TABLE blocks                    ADD COLUMN account_id_new               BIGINT;
ALTER TABLE blocks                    ADD COLUMN target_account_id_new        BIGINT;
ALTER TABLE bookmarks                 ADD COLUMN account_id_new               BIGINT;
ALTER TABLE conversation_mutes        ADD COLUMN account_id_new               BIGINT;
ALTER TABLE conversation_participants ADD COLUMN account_id_new               BIGINT;
ALTER TABLE custom_filters            ADD COLUMN account_id_new               BIGINT;
ALTER TABLE favourites                ADD COLUMN account_id_new               BIGINT;
ALTER TABLE featured_tags             ADD COLUMN account_id_new               BIGINT;
ALTER TABLE follows                   ADD COLUMN account_id_new               BIGINT;
ALTER TABLE follows                   ADD COLUMN target_account_id_new        BIGINT;
ALTER TABLE invites                   ADD COLUMN created_by_new               BIGINT;
ALTER TABLE list_accounts             ADD COLUMN account_id_new               BIGINT;
ALTER TABLE lists                     ADD COLUMN account_id_new               BIGINT;
ALTER TABLE markers                   ADD COLUMN account_id_new               BIGINT;
ALTER TABLE media_attachments         ADD COLUMN account_id_new               BIGINT;
ALTER TABLE mentions                  ADD COLUMN account_id_new               BIGINT;
ALTER TABLE mutes                     ADD COLUMN account_id_new               BIGINT;
ALTER TABLE mutes                     ADD COLUMN target_account_id_new        BIGINT;
ALTER TABLE notification_requests     ADD COLUMN account_id_new               BIGINT;
ALTER TABLE notification_requests     ADD COLUMN from_account_id_new          BIGINT;
ALTER TABLE notifications             ADD COLUMN account_id_new               BIGINT;
ALTER TABLE notifications             ADD COLUMN from_account_id_new          BIGINT;
ALTER TABLE oauth_access_tokens       ADD COLUMN account_id_new               BIGINT;
ALTER TABLE oauth_authorization_codes ADD COLUMN account_id_new               BIGINT;
ALTER TABLE outbox_queue              ADD COLUMN account_id_new               BIGINT;
ALTER TABLE poll_votes                ADD COLUMN account_id_new               BIGINT;
ALTER TABLE polls                     ADD COLUMN account_id_new               BIGINT;
ALTER TABLE report_notes              ADD COLUMN account_id_new               BIGINT;
ALTER TABLE reports                   ADD COLUMN account_id_new               BIGINT;
ALTER TABLE reports                   ADD COLUMN action_taken_by_account_id_new BIGINT;
ALTER TABLE reports                   ADD COLUMN assigned_account_id_new      BIGINT;
ALTER TABLE reports                   ADD COLUMN target_account_id_new        BIGINT;
ALTER TABLE scheduled_statuses        ADD COLUMN account_id_new               BIGINT;
ALTER TABLE status_edits              ADD COLUMN account_id_new               BIGINT;
ALTER TABLE status_pins               ADD COLUMN account_id_new               BIGINT;
ALTER TABLE statuses                  ADD COLUMN account_id_new               BIGINT;
ALTER TABLE statuses                  ADD COLUMN in_reply_to_account_id_new   BIGINT;
ALTER TABLE suggestion_dismissals     ADD COLUMN account_id_new               BIGINT;
ALTER TABLE suggestion_dismissals     ADD COLUMN target_account_id_new        BIGINT;
ALTER TABLE tag_follows               ADD COLUMN account_id_new               BIGINT;
ALTER TABLE user_domain_blocks        ADD COLUMN account_id_new               BIGINT;
ALTER TABLE users                     ADD COLUMN account_id_new               BIGINT;
ALTER TABLE web_push_subscriptions    ADD COLUMN account_id_new               BIGINT;

-- ── Step 4: Populate BIGINT shadow columns ───────────────────────────────────
UPDATE account_aliases           SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE account_moderation_notes  SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     target_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = target_account_id);
UPDATE account_notes             SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     target_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = target_account_id);
UPDATE account_pins              SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     target_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = target_account_id);
UPDATE account_warnings          SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     target_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = target_account_id);
UPDATE admin_action_logs         SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE announcement_dismissals   SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE blocks                    SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     target_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = target_account_id);
UPDATE bookmarks                 SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE conversation_mutes        SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE conversation_participants SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE custom_filters            SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE favourites                SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE featured_tags             SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE follows                   SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     target_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = target_account_id);
UPDATE invites                   SET created_by_new = (SELECT new_id FROM _acct_map WHERE old_id = created_by);
UPDATE list_accounts             SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE lists                     SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE markers                   SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE media_attachments         SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE mentions                  SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE mutes                     SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     target_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = target_account_id);
UPDATE notification_requests     SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     from_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = from_account_id);
UPDATE notifications             SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     from_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = from_account_id);
UPDATE oauth_access_tokens       SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE oauth_authorization_codes SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE outbox_queue              SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE poll_votes                SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE polls                     SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE report_notes              SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE reports                   SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     action_taken_by_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = action_taken_by_account_id),
                                     assigned_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = assigned_account_id),
                                     target_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = target_account_id);
UPDATE scheduled_statuses        SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE status_edits              SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE status_pins               SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE statuses                  SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     in_reply_to_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = in_reply_to_account_id);
UPDATE suggestion_dismissals     SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id),
                                     target_account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = target_account_id);
UPDATE tag_follows               SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE user_domain_blocks        SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE users                     SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);
UPDATE web_push_subscriptions    SET account_id_new = (SELECT new_id FROM _acct_map WHERE old_id = account_id);

-- ── Step 5: Drop all FK constraints referencing accounts.id ──────────────────
ALTER TABLE account_aliases           DROP CONSTRAINT account_aliases_account_id_fkey;
ALTER TABLE account_moderation_notes  DROP CONSTRAINT account_moderation_notes_account_id_fkey;
ALTER TABLE account_moderation_notes  DROP CONSTRAINT account_moderation_notes_target_account_id_fkey;
ALTER TABLE account_notes             DROP CONSTRAINT account_notes_account_id_fkey;
ALTER TABLE account_notes             DROP CONSTRAINT account_notes_target_account_id_fkey;
ALTER TABLE account_pins              DROP CONSTRAINT account_pins_account_id_fkey;
ALTER TABLE account_pins              DROP CONSTRAINT account_pins_target_account_id_fkey;
ALTER TABLE account_warnings          DROP CONSTRAINT account_warnings_account_id_fkey;
ALTER TABLE account_warnings          DROP CONSTRAINT account_warnings_target_account_id_fkey;
ALTER TABLE admin_action_logs         DROP CONSTRAINT admin_action_logs_account_id_fkey;
ALTER TABLE announcement_dismissals   DROP CONSTRAINT announcement_dismissals_account_id_fkey;
ALTER TABLE blocks                    DROP CONSTRAINT blocks_account_id_fkey;
ALTER TABLE blocks                    DROP CONSTRAINT blocks_target_account_id_fkey;
ALTER TABLE bookmarks                 DROP CONSTRAINT bookmarks_account_id_fkey;
ALTER TABLE conversation_mutes        DROP CONSTRAINT conversation_mutes_account_id_fkey;
ALTER TABLE conversation_participants DROP CONSTRAINT conversation_participants_account_id_fkey;
ALTER TABLE custom_filters            DROP CONSTRAINT custom_filters_account_id_fkey;
ALTER TABLE favourites                DROP CONSTRAINT favourites_account_id_fkey;
ALTER TABLE featured_tags             DROP CONSTRAINT featured_tags_account_id_fkey;
ALTER TABLE follows                   DROP CONSTRAINT follows_account_id_fkey;
ALTER TABLE follows                   DROP CONSTRAINT follows_target_account_id_fkey;
ALTER TABLE invites                   DROP CONSTRAINT invites_created_by_fkey;
ALTER TABLE list_accounts             DROP CONSTRAINT list_accounts_account_id_fkey;
ALTER TABLE lists                     DROP CONSTRAINT lists_account_id_fkey;
ALTER TABLE markers                   DROP CONSTRAINT markers_account_id_fkey;
ALTER TABLE media_attachments         DROP CONSTRAINT media_attachments_account_id_fkey;
ALTER TABLE mentions                  DROP CONSTRAINT mentions_account_id_fkey;
ALTER TABLE mutes                     DROP CONSTRAINT mutes_account_id_fkey;
ALTER TABLE mutes                     DROP CONSTRAINT mutes_target_account_id_fkey;
ALTER TABLE notification_requests     DROP CONSTRAINT notification_requests_account_id_fkey;
ALTER TABLE notification_requests     DROP CONSTRAINT notification_requests_from_account_id_fkey;
ALTER TABLE notifications             DROP CONSTRAINT notifications_account_id_fkey;
ALTER TABLE notifications             DROP CONSTRAINT notifications_from_account_id_fkey;
ALTER TABLE oauth_access_tokens       DROP CONSTRAINT oauth_access_tokens_account_id_fkey;
ALTER TABLE oauth_authorization_codes DROP CONSTRAINT oauth_authorization_codes_account_id_fkey;
ALTER TABLE outbox_queue              DROP CONSTRAINT outbox_queue_account_id_fkey;
ALTER TABLE poll_votes                DROP CONSTRAINT poll_votes_account_id_fkey;
ALTER TABLE polls                     DROP CONSTRAINT polls_account_id_fkey;
ALTER TABLE report_notes              DROP CONSTRAINT report_notes_account_id_fkey;
ALTER TABLE reports                   DROP CONSTRAINT reports_account_id_fkey;
ALTER TABLE reports                   DROP CONSTRAINT reports_action_taken_by_account_id_fkey;
ALTER TABLE reports                   DROP CONSTRAINT reports_assigned_account_id_fkey;
ALTER TABLE reports                   DROP CONSTRAINT reports_target_account_id_fkey;
ALTER TABLE scheduled_statuses        DROP CONSTRAINT scheduled_statuses_account_id_fkey;
ALTER TABLE status_edits              DROP CONSTRAINT status_edits_account_id_fkey;
ALTER TABLE status_pins               DROP CONSTRAINT status_pins_account_id_fkey;
ALTER TABLE statuses                  DROP CONSTRAINT statuses_account_id_fkey;
ALTER TABLE statuses                  DROP CONSTRAINT statuses_in_reply_to_account_id_fkey;
ALTER TABLE suggestion_dismissals     DROP CONSTRAINT suggestion_dismissals_account_id_fkey;
ALTER TABLE suggestion_dismissals     DROP CONSTRAINT suggestion_dismissals_target_account_id_fkey;
ALTER TABLE tag_follows               DROP CONSTRAINT tag_follows_account_id_fkey;
ALTER TABLE user_domain_blocks        DROP CONSTRAINT user_domain_blocks_account_id_fkey;
ALTER TABLE users                     DROP CONSTRAINT users_account_id_fkey;
ALTER TABLE web_push_subscriptions    DROP CONSTRAINT web_push_subscriptions_account_id_fkey;

-- ── Step 6: Drop composite PKs that include account_id ───────────────────────
ALTER TABLE announcement_dismissals   DROP CONSTRAINT announcement_dismissals_pkey;
ALTER TABLE conversation_participants DROP CONSTRAINT conversation_participants_pkey;

-- ── Step 7: Drop accounts PK and old id column, rename new ───────────────────
ALTER TABLE accounts DROP CONSTRAINT accounts_pkey;
ALTER TABLE accounts DROP COLUMN id;          -- cascades to accounts_pkey index
ALTER TABLE accounts RENAME COLUMN id_new TO id;
ALTER TABLE accounts ALTER COLUMN id SET NOT NULL;
ALTER TABLE accounts ADD PRIMARY KEY (id);

-- ── Step 8: Swap columns in every referencing table ──────────────────────────
-- account_aliases
ALTER TABLE account_aliases DROP COLUMN account_id;
ALTER TABLE account_aliases RENAME COLUMN account_id_new TO account_id;
ALTER TABLE account_aliases ALTER COLUMN account_id SET NOT NULL;

-- account_moderation_notes
ALTER TABLE account_moderation_notes DROP COLUMN account_id;
ALTER TABLE account_moderation_notes RENAME COLUMN account_id_new TO account_id;
ALTER TABLE account_moderation_notes ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE account_moderation_notes DROP COLUMN target_account_id;
ALTER TABLE account_moderation_notes RENAME COLUMN target_account_id_new TO target_account_id;
ALTER TABLE account_moderation_notes ALTER COLUMN target_account_id SET NOT NULL;

-- account_notes
ALTER TABLE account_notes DROP COLUMN account_id;
ALTER TABLE account_notes RENAME COLUMN account_id_new TO account_id;
ALTER TABLE account_notes ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE account_notes DROP COLUMN target_account_id;
ALTER TABLE account_notes RENAME COLUMN target_account_id_new TO target_account_id;
ALTER TABLE account_notes ALTER COLUMN target_account_id SET NOT NULL;

-- account_pins
ALTER TABLE account_pins DROP COLUMN account_id;
ALTER TABLE account_pins RENAME COLUMN account_id_new TO account_id;
ALTER TABLE account_pins ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE account_pins DROP COLUMN target_account_id;
ALTER TABLE account_pins RENAME COLUMN target_account_id_new TO target_account_id;
ALTER TABLE account_pins ALTER COLUMN target_account_id SET NOT NULL;

-- account_warnings (account_id is nullable / SET NULL)
ALTER TABLE account_warnings DROP COLUMN account_id;
ALTER TABLE account_warnings RENAME COLUMN account_id_new TO account_id;
ALTER TABLE account_warnings DROP COLUMN target_account_id;
ALTER TABLE account_warnings RENAME COLUMN target_account_id_new TO target_account_id;
ALTER TABLE account_warnings ALTER COLUMN target_account_id SET NOT NULL;

-- admin_action_logs
ALTER TABLE admin_action_logs DROP COLUMN account_id;
ALTER TABLE admin_action_logs RENAME COLUMN account_id_new TO account_id;
ALTER TABLE admin_action_logs ALTER COLUMN account_id SET NOT NULL;

-- announcement_dismissals
ALTER TABLE announcement_dismissals DROP COLUMN account_id;
ALTER TABLE announcement_dismissals RENAME COLUMN account_id_new TO account_id;
ALTER TABLE announcement_dismissals ALTER COLUMN account_id SET NOT NULL;

-- blocks
ALTER TABLE blocks DROP COLUMN account_id;
ALTER TABLE blocks RENAME COLUMN account_id_new TO account_id;
ALTER TABLE blocks ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE blocks DROP COLUMN target_account_id;
ALTER TABLE blocks RENAME COLUMN target_account_id_new TO target_account_id;
ALTER TABLE blocks ALTER COLUMN target_account_id SET NOT NULL;

-- bookmarks
ALTER TABLE bookmarks DROP COLUMN account_id;
ALTER TABLE bookmarks RENAME COLUMN account_id_new TO account_id;
ALTER TABLE bookmarks ALTER COLUMN account_id SET NOT NULL;

-- conversation_mutes
ALTER TABLE conversation_mutes DROP COLUMN account_id;
ALTER TABLE conversation_mutes RENAME COLUMN account_id_new TO account_id;
ALTER TABLE conversation_mutes ALTER COLUMN account_id SET NOT NULL;

-- conversation_participants
ALTER TABLE conversation_participants DROP COLUMN account_id;
ALTER TABLE conversation_participants RENAME COLUMN account_id_new TO account_id;
ALTER TABLE conversation_participants ALTER COLUMN account_id SET NOT NULL;

-- custom_filters
ALTER TABLE custom_filters DROP COLUMN account_id;
ALTER TABLE custom_filters RENAME COLUMN account_id_new TO account_id;
ALTER TABLE custom_filters ALTER COLUMN account_id SET NOT NULL;

-- favourites
ALTER TABLE favourites DROP COLUMN account_id;
ALTER TABLE favourites RENAME COLUMN account_id_new TO account_id;
ALTER TABLE favourites ALTER COLUMN account_id SET NOT NULL;

-- featured_tags
ALTER TABLE featured_tags DROP COLUMN account_id;
ALTER TABLE featured_tags RENAME COLUMN account_id_new TO account_id;
ALTER TABLE featured_tags ALTER COLUMN account_id SET NOT NULL;

-- follows
ALTER TABLE follows DROP COLUMN account_id;
ALTER TABLE follows RENAME COLUMN account_id_new TO account_id;
ALTER TABLE follows ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE follows DROP COLUMN target_account_id;
ALTER TABLE follows RENAME COLUMN target_account_id_new TO target_account_id;
ALTER TABLE follows ALTER COLUMN target_account_id SET NOT NULL;

-- invites (created_by is nullable / SET NULL)
ALTER TABLE invites DROP COLUMN created_by;
ALTER TABLE invites RENAME COLUMN created_by_new TO created_by;

-- list_accounts
ALTER TABLE list_accounts DROP COLUMN account_id;
ALTER TABLE list_accounts RENAME COLUMN account_id_new TO account_id;
ALTER TABLE list_accounts ALTER COLUMN account_id SET NOT NULL;

-- lists
ALTER TABLE lists DROP COLUMN account_id;
ALTER TABLE lists RENAME COLUMN account_id_new TO account_id;
ALTER TABLE lists ALTER COLUMN account_id SET NOT NULL;

-- markers
ALTER TABLE markers DROP COLUMN account_id;
ALTER TABLE markers RENAME COLUMN account_id_new TO account_id;
ALTER TABLE markers ALTER COLUMN account_id SET NOT NULL;

-- media_attachments
ALTER TABLE media_attachments DROP COLUMN account_id;
ALTER TABLE media_attachments RENAME COLUMN account_id_new TO account_id;
ALTER TABLE media_attachments ALTER COLUMN account_id SET NOT NULL;

-- mentions
ALTER TABLE mentions DROP COLUMN account_id;
ALTER TABLE mentions RENAME COLUMN account_id_new TO account_id;
ALTER TABLE mentions ALTER COLUMN account_id SET NOT NULL;

-- mutes
ALTER TABLE mutes DROP COLUMN account_id;
ALTER TABLE mutes RENAME COLUMN account_id_new TO account_id;
ALTER TABLE mutes ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE mutes DROP COLUMN target_account_id;
ALTER TABLE mutes RENAME COLUMN target_account_id_new TO target_account_id;
ALTER TABLE mutes ALTER COLUMN target_account_id SET NOT NULL;

-- notification_requests
ALTER TABLE notification_requests DROP COLUMN account_id;
ALTER TABLE notification_requests RENAME COLUMN account_id_new TO account_id;
ALTER TABLE notification_requests ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE notification_requests DROP COLUMN from_account_id;
ALTER TABLE notification_requests RENAME COLUMN from_account_id_new TO from_account_id;
ALTER TABLE notification_requests ALTER COLUMN from_account_id SET NOT NULL;

-- notifications
ALTER TABLE notifications DROP COLUMN account_id;
ALTER TABLE notifications RENAME COLUMN account_id_new TO account_id;
ALTER TABLE notifications ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE notifications DROP COLUMN from_account_id;
ALTER TABLE notifications RENAME COLUMN from_account_id_new TO from_account_id;
ALTER TABLE notifications ALTER COLUMN from_account_id SET NOT NULL;

-- oauth_access_tokens (account_id is nullable)
ALTER TABLE oauth_access_tokens DROP COLUMN account_id;
ALTER TABLE oauth_access_tokens RENAME COLUMN account_id_new TO account_id;

-- oauth_authorization_codes (account_id is nullable)
ALTER TABLE oauth_authorization_codes DROP COLUMN account_id;
ALTER TABLE oauth_authorization_codes RENAME COLUMN account_id_new TO account_id;

-- outbox_queue
ALTER TABLE outbox_queue DROP COLUMN account_id;
ALTER TABLE outbox_queue RENAME COLUMN account_id_new TO account_id;
ALTER TABLE outbox_queue ALTER COLUMN account_id SET NOT NULL;

-- poll_votes
ALTER TABLE poll_votes DROP COLUMN account_id;
ALTER TABLE poll_votes RENAME COLUMN account_id_new TO account_id;
ALTER TABLE poll_votes ALTER COLUMN account_id SET NOT NULL;

-- polls
ALTER TABLE polls DROP COLUMN account_id;
ALTER TABLE polls RENAME COLUMN account_id_new TO account_id;
ALTER TABLE polls ALTER COLUMN account_id SET NOT NULL;

-- report_notes
ALTER TABLE report_notes DROP COLUMN account_id;
ALTER TABLE report_notes RENAME COLUMN account_id_new TO account_id;
ALTER TABLE report_notes ALTER COLUMN account_id SET NOT NULL;

-- reports (some nullable / SET NULL)
ALTER TABLE reports DROP COLUMN account_id;
ALTER TABLE reports RENAME COLUMN account_id_new TO account_id;
ALTER TABLE reports ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE reports DROP COLUMN action_taken_by_account_id;
ALTER TABLE reports RENAME COLUMN action_taken_by_account_id_new TO action_taken_by_account_id;
ALTER TABLE reports DROP COLUMN assigned_account_id;
ALTER TABLE reports RENAME COLUMN assigned_account_id_new TO assigned_account_id;
ALTER TABLE reports DROP COLUMN target_account_id;
ALTER TABLE reports RENAME COLUMN target_account_id_new TO target_account_id;
ALTER TABLE reports ALTER COLUMN target_account_id SET NOT NULL;

-- scheduled_statuses
ALTER TABLE scheduled_statuses DROP COLUMN account_id;
ALTER TABLE scheduled_statuses RENAME COLUMN account_id_new TO account_id;
ALTER TABLE scheduled_statuses ALTER COLUMN account_id SET NOT NULL;

-- status_edits (account_id is nullable / SET NULL)
ALTER TABLE status_edits DROP COLUMN account_id;
ALTER TABLE status_edits RENAME COLUMN account_id_new TO account_id;

-- status_pins
ALTER TABLE status_pins DROP COLUMN account_id;
ALTER TABLE status_pins RENAME COLUMN account_id_new TO account_id;
ALTER TABLE status_pins ALTER COLUMN account_id SET NOT NULL;

-- statuses (in_reply_to_account_id is nullable / SET NULL)
ALTER TABLE statuses DROP COLUMN account_id;
ALTER TABLE statuses RENAME COLUMN account_id_new TO account_id;
ALTER TABLE statuses ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE statuses DROP COLUMN in_reply_to_account_id;
ALTER TABLE statuses RENAME COLUMN in_reply_to_account_id_new TO in_reply_to_account_id;

-- suggestion_dismissals
ALTER TABLE suggestion_dismissals DROP COLUMN account_id;
ALTER TABLE suggestion_dismissals RENAME COLUMN account_id_new TO account_id;
ALTER TABLE suggestion_dismissals ALTER COLUMN account_id SET NOT NULL;
ALTER TABLE suggestion_dismissals DROP COLUMN target_account_id;
ALTER TABLE suggestion_dismissals RENAME COLUMN target_account_id_new TO target_account_id;
ALTER TABLE suggestion_dismissals ALTER COLUMN target_account_id SET NOT NULL;

-- tag_follows
ALTER TABLE tag_follows DROP COLUMN account_id;
ALTER TABLE tag_follows RENAME COLUMN account_id_new TO account_id;
ALTER TABLE tag_follows ALTER COLUMN account_id SET NOT NULL;

-- user_domain_blocks
ALTER TABLE user_domain_blocks DROP COLUMN account_id;
ALTER TABLE user_domain_blocks RENAME COLUMN account_id_new TO account_id;
ALTER TABLE user_domain_blocks ALTER COLUMN account_id SET NOT NULL;

-- users
ALTER TABLE users DROP COLUMN account_id;
ALTER TABLE users RENAME COLUMN account_id_new TO account_id;
ALTER TABLE users ALTER COLUMN account_id SET NOT NULL;

-- web_push_subscriptions
ALTER TABLE web_push_subscriptions DROP COLUMN account_id;
ALTER TABLE web_push_subscriptions RENAME COLUMN account_id_new TO account_id;
ALTER TABLE web_push_subscriptions ALTER COLUMN account_id SET NOT NULL;

-- ── Step 9: Restore composite PKs ────────────────────────────────────────────
ALTER TABLE announcement_dismissals   ADD PRIMARY KEY (announcement_id, account_id);
ALTER TABLE conversation_participants ADD PRIMARY KEY (conversation_id, account_id);

-- ── Step 10: Recreate FK constraints ─────────────────────────────────────────
ALTER TABLE account_aliases           ADD CONSTRAINT account_aliases_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE account_moderation_notes  ADD CONSTRAINT account_moderation_notes_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE account_moderation_notes  ADD CONSTRAINT account_moderation_notes_target_account_id_fkey
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE account_notes             ADD CONSTRAINT account_notes_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE account_notes             ADD CONSTRAINT account_notes_target_account_id_fkey
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE account_pins              ADD CONSTRAINT account_pins_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE account_pins              ADD CONSTRAINT account_pins_target_account_id_fkey
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE account_warnings          ADD CONSTRAINT account_warnings_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE SET NULL;
ALTER TABLE account_warnings          ADD CONSTRAINT account_warnings_target_account_id_fkey
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE admin_action_logs         ADD CONSTRAINT admin_action_logs_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE announcement_dismissals   ADD CONSTRAINT announcement_dismissals_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE blocks                    ADD CONSTRAINT blocks_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE blocks                    ADD CONSTRAINT blocks_target_account_id_fkey
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE bookmarks                 ADD CONSTRAINT bookmarks_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE conversation_mutes        ADD CONSTRAINT conversation_mutes_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE conversation_participants ADD CONSTRAINT conversation_participants_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE custom_filters            ADD CONSTRAINT custom_filters_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE favourites                ADD CONSTRAINT favourites_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE featured_tags             ADD CONSTRAINT featured_tags_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE follows                   ADD CONSTRAINT follows_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE follows                   ADD CONSTRAINT follows_target_account_id_fkey
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE invites                   ADD CONSTRAINT invites_created_by_fkey
    FOREIGN KEY (created_by) REFERENCES accounts(id) ON DELETE SET NULL;
ALTER TABLE list_accounts             ADD CONSTRAINT list_accounts_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE lists                     ADD CONSTRAINT lists_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE markers                   ADD CONSTRAINT markers_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE media_attachments         ADD CONSTRAINT media_attachments_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE mentions                  ADD CONSTRAINT mentions_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE mutes                     ADD CONSTRAINT mutes_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE mutes                     ADD CONSTRAINT mutes_target_account_id_fkey
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE notification_requests     ADD CONSTRAINT notification_requests_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE notification_requests     ADD CONSTRAINT notification_requests_from_account_id_fkey
    FOREIGN KEY (from_account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE notifications             ADD CONSTRAINT notifications_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE notifications             ADD CONSTRAINT notifications_from_account_id_fkey
    FOREIGN KEY (from_account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE oauth_access_tokens       ADD CONSTRAINT oauth_access_tokens_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE oauth_authorization_codes ADD CONSTRAINT oauth_authorization_codes_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE outbox_queue              ADD CONSTRAINT outbox_queue_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE poll_votes                ADD CONSTRAINT poll_votes_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE polls                     ADD CONSTRAINT polls_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE report_notes              ADD CONSTRAINT report_notes_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE reports                   ADD CONSTRAINT reports_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE reports                   ADD CONSTRAINT reports_action_taken_by_account_id_fkey
    FOREIGN KEY (action_taken_by_account_id) REFERENCES accounts(id) ON DELETE SET NULL;
ALTER TABLE reports                   ADD CONSTRAINT reports_assigned_account_id_fkey
    FOREIGN KEY (assigned_account_id) REFERENCES accounts(id) ON DELETE SET NULL;
ALTER TABLE reports                   ADD CONSTRAINT reports_target_account_id_fkey
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE scheduled_statuses        ADD CONSTRAINT scheduled_statuses_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE status_edits              ADD CONSTRAINT status_edits_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE SET NULL;
ALTER TABLE status_pins               ADD CONSTRAINT status_pins_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE statuses                  ADD CONSTRAINT statuses_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE statuses                  ADD CONSTRAINT statuses_in_reply_to_account_id_fkey
    FOREIGN KEY (in_reply_to_account_id) REFERENCES accounts(id) ON DELETE SET NULL;
ALTER TABLE suggestion_dismissals     ADD CONSTRAINT suggestion_dismissals_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE suggestion_dismissals     ADD CONSTRAINT suggestion_dismissals_target_account_id_fkey
    FOREIGN KEY (target_account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE tag_follows               ADD CONSTRAINT tag_follows_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE user_domain_blocks        ADD CONSTRAINT user_domain_blocks_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE users                     ADD CONSTRAINT users_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;
ALTER TABLE web_push_subscriptions    ADD CONSTRAINT web_push_subscriptions_account_id_fkey
    FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE;

-- ── Step 11: Recreate indices ─────────────────────────────────────────────────
CREATE UNIQUE INDEX account_aliases_account_id_uri_key
    ON account_aliases (account_id, uri);
CREATE UNIQUE INDEX account_notes_account_id_target_account_id_key
    ON account_notes (account_id, target_account_id);
CREATE UNIQUE INDEX account_pins_account_id_target_account_id_key
    ON account_pins (account_id, target_account_id);
CREATE UNIQUE INDEX blocks_account_id_target_account_id_key
    ON blocks (account_id, target_account_id);
CREATE UNIQUE INDEX bookmarks_account_id_status_id_key
    ON bookmarks (account_id, status_id);
CREATE UNIQUE INDEX conversation_mutes_account_id_status_id_key
    ON conversation_mutes (account_id, status_id);
CREATE UNIQUE INDEX favourites_account_id_status_id_key
    ON favourites (account_id, status_id);
CREATE UNIQUE INDEX featured_tags_account_id_tag_id_key
    ON featured_tags (account_id, tag_id);
CREATE UNIQUE INDEX follows_account_id_target_account_id_key
    ON follows (account_id, target_account_id);
CREATE INDEX follows_by_target
    ON follows (target_account_id) WHERE state = 'accepted';
CREATE UNIQUE INDEX list_accounts_list_id_account_id_key
    ON list_accounts (list_id, account_id);
CREATE UNIQUE INDEX markers_account_id_timeline_key
    ON markers (account_id, timeline);
CREATE INDEX media_by_account
    ON media_attachments (account_id);
CREATE UNIQUE INDEX mentions_status_id_account_id_key
    ON mentions (status_id, account_id);
CREATE UNIQUE INDEX mutes_account_id_target_account_id_key
    ON mutes (account_id, target_account_id);
CREATE UNIQUE INDEX notification_requests_account_id_from_account_id_key
    ON notification_requests (account_id, from_account_id);
CREATE INDEX notifications_by_account
    ON notifications (account_id, id DESC);
CREATE INDEX tokens_by_account
    ON oauth_access_tokens (account_id) WHERE revoked_at IS NULL;
CREATE UNIQUE INDEX poll_votes_poll_id_account_id_choice_key
    ON poll_votes (poll_id, account_id, choice);
CREATE UNIQUE INDEX status_pins_account_id_status_id_key
    ON status_pins (account_id, status_id);
CREATE INDEX statuses_by_account
    ON statuses (account_id, id DESC) WHERE deleted_at IS NULL;
CREATE INDEX statuses_by_reblog
    ON statuses (account_id, reblog_of_id) WHERE reblog_of_id IS NOT NULL AND deleted_at IS NULL;
CREATE UNIQUE INDEX statuses_idempotency_key_idx
    ON statuses (account_id, idempotency_key) WHERE idempotency_key IS NOT NULL;
CREATE UNIQUE INDEX suggestion_dismissals_account_id_target_account_id_key
    ON suggestion_dismissals (account_id, target_account_id);
CREATE UNIQUE INDEX tag_follows_account_id_tag_id_key
    ON tag_follows (account_id, tag_id);
CREATE INDEX tag_follows_by_account
    ON tag_follows (account_id);
CREATE UNIQUE INDEX user_domain_blocks_account_id_domain_key
    ON user_domain_blocks (account_id, domain);
CREATE UNIQUE INDEX users_account_id_key
    ON users (account_id);

COMMIT;
