-- Migration 076: Complete the UUID→bigint conversion blocked by the user_ips view.
-- Migration 075 partially failed because user_ips is a VIEW that depends on
-- users.id, login_activities.user_id, and session_activations.user_id.

-- ── Step 1: Drop user_ips view (will be recreated at end) ───────────────────
DROP VIEW IF EXISTS user_ips;

-- ── Step 2: Finalize users.id conversion ────────────────────────────────────
-- users.new_id (bigint) was already populated in migration 075.
ALTER TABLE users DROP CONSTRAINT users_pkey;
ALTER TABLE users DROP COLUMN id;
ALTER TABLE users RENAME COLUMN new_id TO id;
ALTER TABLE users ADD PRIMARY KEY (id);
CREATE SEQUENCE users_id_seq;
SELECT setval('users_id_seq', (SELECT MAX(id) FROM users));
ALTER TABLE users ALTER COLUMN id SET DEFAULT nextval('users_id_seq');
ALTER SEQUENCE users_id_seq OWNED BY users.id;

-- ── Step 3: Finalize login_activities.user_id conversion ────────────────────
-- login_activities.user_id_new (bigint) was already populated.
ALTER TABLE login_activities DROP COLUMN user_id;
ALTER TABLE login_activities RENAME COLUMN user_id_new TO user_id;
CREATE INDEX index_login_activities_on_user_id ON login_activities (user_id);

-- ── Step 4: Finalize session_activations.user_id conversion ─────────────────
-- session_activations.user_id_new (bigint) was already populated.
ALTER TABLE session_activations DROP COLUMN user_id;
ALTER TABLE session_activations RENAME COLUMN user_id_new TO user_id;
ALTER TABLE session_activations ALTER COLUMN user_id SET NOT NULL;
CREATE INDEX index_session_activations_on_user_id ON session_activations (user_id);

-- ── Step 5: Recreate user_ips view with bigint user_id ──────────────────────
CREATE VIEW user_ips AS
    SELECT user_id, ip, MAX(used_at) AS used_at
    FROM (
        SELECT u.id AS user_id, u.sign_up_ip AS ip, u.created_at AS used_at
        FROM users u WHERE u.sign_up_ip IS NOT NULL
        UNION ALL
        SELECT sa.user_id, sa.ip, sa.updated_at
        FROM session_activations sa WHERE sa.ip IS NOT NULL
        UNION ALL
        SELECT la.user_id, la.ip, la.created_at
        FROM login_activities la WHERE la.ip IS NOT NULL AND la.success = true
    ) t
    GROUP BY user_id, ip;

-- ── Step 6: Recreate FK constraints that failed in migration 075 ─────────────
ALTER TABLE backups ADD CONSTRAINT backups_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE identities ADD CONSTRAINT identities_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE instance_user_sessions ADD CONSTRAINT instance_user_sessions_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE invites ADD CONSTRAINT invites_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE markers ADD CONSTRAINT markers_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE oauth_access_tokens ADD CONSTRAINT oauth_access_tokens_resource_owner_id_fkey
    FOREIGN KEY (resource_owner_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE session_activations ADD CONSTRAINT session_activations_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE user_invite_requests ADD CONSTRAINT user_invite_requests_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE web_push_subscriptions ADD CONSTRAINT web_push_subscriptions_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE web_settings ADD CONSTRAINT web_settings_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE webauthn_credentials ADD CONSTRAINT webauthn_credentials_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE login_activities ADD CONSTRAINT login_activities_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
