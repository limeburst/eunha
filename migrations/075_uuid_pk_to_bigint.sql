-- Migration 075: Convert UUID primary keys to bigint for Mastodon schema parity.
-- Tables preserved as UUID (intentional eunha design):
--   instances, console_users, console_sessions, instance_user_sessions, pending_signups

-- ── Phase 0: Drop all FK constraints that cross UUID→bigint boundaries ───────

ALTER TABLE list_accounts          DROP CONSTRAINT list_accounts_follow_id_fkey;
ALTER TABLE announcement_reactions DROP CONSTRAINT announcement_reactions_custom_emoji_id_fkey;
ALTER TABLE accounts_tags          DROP CONSTRAINT accounts_tags_tag_id_fkey;
ALTER TABLE featured_tags          DROP CONSTRAINT featured_tags_tag_id_fkey;
ALTER TABLE statuses_tags          DROP CONSTRAINT statuses_tags_tag_id_fkey;
ALTER TABLE tag_follows            DROP CONSTRAINT tag_follows_tag_id_fkey;
ALTER TABLE tag_trends             DROP CONSTRAINT tag_trends_tag_id_fkey;
ALTER TABLE poll_votes             DROP CONSTRAINT poll_votes_poll_id_fkey;
ALTER TABLE users                  DROP CONSTRAINT users_invite_id_fkey;
ALTER TABLE pending_signups        DROP CONSTRAINT pending_signups_invite_id_fkey;
ALTER TABLE session_activations    DROP CONSTRAINT session_activations_access_token_id_fkey;
ALTER TABLE web_push_subscriptions DROP CONSTRAINT web_push_subscriptions_access_token_id_fkey;
ALTER TABLE backups                DROP CONSTRAINT backups_user_id_fkey;
ALTER TABLE identities             DROP CONSTRAINT identities_user_id_fkey;
ALTER TABLE instance_user_sessions DROP CONSTRAINT instance_user_sessions_user_id_fkey;
ALTER TABLE invites                DROP CONSTRAINT invites_user_id_fkey;
ALTER TABLE login_activities       DROP CONSTRAINT login_activities_user_id_fkey;
ALTER TABLE markers                DROP CONSTRAINT markers_user_id_fkey;
ALTER TABLE oauth_access_tokens    DROP CONSTRAINT oauth_access_tokens_resource_owner_id_fkey;
ALTER TABLE session_activations    DROP CONSTRAINT session_activations_user_id_fkey;
ALTER TABLE user_invite_requests   DROP CONSTRAINT user_invite_requests_user_id_fkey;
ALTER TABLE web_push_subscriptions DROP CONSTRAINT web_push_subscriptions_user_id_fkey;
ALTER TABLE web_settings           DROP CONSTRAINT web_settings_user_id_fkey;
ALTER TABLE webauthn_credentials   DROP CONSTRAINT webauthn_credentials_user_id_fkey;

-- ── Phase 1: Simple tables — no FK references to their PK ───────────────────
-- Pattern: drop old UUID PK, add BIGSERIAL. Existing rows get new sequential IDs.
-- These IDs are not referenced by any FK so no child-table remapping needed.

ALTER TABLE blocks DROP CONSTRAINT blocks_pkey;
ALTER TABLE blocks DROP COLUMN id;
ALTER TABLE blocks ADD COLUMN id BIGSERIAL PRIMARY KEY;

ALTER TABLE bookmarks DROP CONSTRAINT bookmarks_pkey;
ALTER TABLE bookmarks DROP COLUMN id;
ALTER TABLE bookmarks ADD COLUMN id BIGSERIAL PRIMARY KEY;

ALTER TABLE favourites DROP CONSTRAINT favourites_pkey;
ALTER TABLE favourites DROP COLUMN id;
ALTER TABLE favourites ADD COLUMN id BIGSERIAL PRIMARY KEY;

-- markers.id (user_id FK handled in Phase 6)
ALTER TABLE markers DROP CONSTRAINT markers_pkey;
ALTER TABLE markers DROP COLUMN id;
ALTER TABLE markers ADD COLUMN id BIGSERIAL PRIMARY KEY;

ALTER TABLE mentions DROP CONSTRAINT mentions_pkey;
ALTER TABLE mentions DROP COLUMN id;
ALTER TABLE mentions ADD COLUMN id BIGSERIAL PRIMARY KEY;

ALTER TABLE mutes DROP CONSTRAINT mutes_pkey;
ALTER TABLE mutes DROP COLUMN id;
ALTER TABLE mutes ADD COLUMN id BIGSERIAL PRIMARY KEY;

ALTER TABLE oauth_access_grants DROP CONSTRAINT oauth_access_grants_pkey;
ALTER TABLE oauth_access_grants DROP COLUMN id;
ALTER TABLE oauth_access_grants ADD COLUMN id BIGSERIAL PRIMARY KEY;

-- poll_votes.id (poll_id FK handled in Phase 5)
ALTER TABLE poll_votes DROP CONSTRAINT poll_votes_pkey;
ALTER TABLE poll_votes DROP COLUMN id;
ALTER TABLE poll_votes ADD COLUMN id BIGSERIAL PRIMARY KEY;

ALTER TABLE status_edits DROP CONSTRAINT status_edits_pkey;
ALTER TABLE status_edits DROP COLUMN id;
ALTER TABLE status_edits ADD COLUMN id BIGSERIAL PRIMARY KEY;

-- ── Phase 2: follows → list_accounts.follow_id ──────────────────────────────

CREATE TEMP TABLE follows_remap AS
    SELECT id AS old_id, ROW_NUMBER() OVER (ORDER BY created_at, id::text) AS new_id
    FROM follows;

ALTER TABLE follows ADD COLUMN new_id BIGINT;
UPDATE follows f SET new_id = r.new_id FROM follows_remap r WHERE r.old_id = f.id;
ALTER TABLE follows ALTER COLUMN new_id SET NOT NULL;

-- list_accounts.follow_id is nullable
ALTER TABLE list_accounts ADD COLUMN follow_id_new BIGINT;
UPDATE list_accounts la SET follow_id_new = r.new_id
    FROM follows_remap r WHERE r.old_id = la.follow_id;
ALTER TABLE list_accounts DROP COLUMN follow_id;
ALTER TABLE list_accounts RENAME COLUMN follow_id_new TO follow_id;

ALTER TABLE follows DROP CONSTRAINT follows_pkey;
ALTER TABLE follows DROP COLUMN id;
ALTER TABLE follows RENAME COLUMN new_id TO id;
ALTER TABLE follows ADD PRIMARY KEY (id);
CREATE SEQUENCE follows_id_seq;
SELECT setval('follows_id_seq', (SELECT COALESCE(MAX(id), 1) FROM follows));
ALTER TABLE follows ALTER COLUMN id SET DEFAULT nextval('follows_id_seq');
ALTER SEQUENCE follows_id_seq OWNED BY follows.id;

-- ── Phase 3: custom_emojis → announcement_reactions.custom_emoji_id ─────────

CREATE TEMP TABLE custom_emojis_remap AS
    SELECT id AS old_id, ROW_NUMBER() OVER (ORDER BY created_at, id::text) AS new_id
    FROM custom_emojis;

ALTER TABLE custom_emojis ADD COLUMN new_id BIGINT;
UPDATE custom_emojis c SET new_id = r.new_id FROM custom_emojis_remap r WHERE r.old_id = c.id;
ALTER TABLE custom_emojis ALTER COLUMN new_id SET NOT NULL;

-- announcement_reactions.custom_emoji_id is nullable
ALTER TABLE announcement_reactions ADD COLUMN custom_emoji_id_new BIGINT;
UPDATE announcement_reactions ar SET custom_emoji_id_new = r.new_id
    FROM custom_emojis_remap r WHERE r.old_id = ar.custom_emoji_id;
ALTER TABLE announcement_reactions DROP COLUMN custom_emoji_id;
ALTER TABLE announcement_reactions RENAME COLUMN custom_emoji_id_new TO custom_emoji_id;

ALTER TABLE custom_emojis DROP CONSTRAINT custom_emojis_pkey;
ALTER TABLE custom_emojis DROP COLUMN id;
ALTER TABLE custom_emojis RENAME COLUMN new_id TO id;
ALTER TABLE custom_emojis ADD PRIMARY KEY (id);
CREATE SEQUENCE custom_emojis_id_seq;
SELECT setval('custom_emojis_id_seq', (SELECT COALESCE(MAX(id), 1) FROM custom_emojis));
ALTER TABLE custom_emojis ALTER COLUMN id SET DEFAULT nextval('custom_emojis_id_seq');
ALTER SEQUENCE custom_emojis_id_seq OWNED BY custom_emojis.id;

-- ── Phase 4: tags → accounts_tags, featured_tags, statuses_tags, ─────────────
--             tag_follows, tag_trends

CREATE TEMP TABLE tags_remap AS
    SELECT id AS old_id, ROW_NUMBER() OVER (ORDER BY created_at, id::text) AS new_id
    FROM tags;

ALTER TABLE tags ADD COLUMN new_id BIGINT;
UPDATE tags t SET new_id = r.new_id FROM tags_remap r WHERE r.old_id = t.id;
ALTER TABLE tags ALTER COLUMN new_id SET NOT NULL;

-- accounts_tags: composite PK (account_id, tag_id)
ALTER TABLE accounts_tags DROP CONSTRAINT accounts_tags_pkey;
DROP INDEX IF EXISTS index_accounts_tags_on_tag_id;
ALTER TABLE accounts_tags ADD COLUMN tag_id_new BIGINT;
UPDATE accounts_tags at SET tag_id_new = r.new_id FROM tags_remap r WHERE r.old_id = at.tag_id;
ALTER TABLE accounts_tags ALTER COLUMN tag_id_new SET NOT NULL;
ALTER TABLE accounts_tags DROP COLUMN tag_id;
ALTER TABLE accounts_tags RENAME COLUMN tag_id_new TO tag_id;
ALTER TABLE accounts_tags ADD PRIMARY KEY (account_id, tag_id);
CREATE INDEX index_accounts_tags_on_tag_id ON accounts_tags (tag_id);

-- featured_tags
DROP INDEX IF EXISTS featured_tags_account_id_tag_id_key;
ALTER TABLE featured_tags ADD COLUMN tag_id_new BIGINT;
UPDATE featured_tags ft SET tag_id_new = r.new_id FROM tags_remap r WHERE r.old_id = ft.tag_id;
ALTER TABLE featured_tags ALTER COLUMN tag_id_new SET NOT NULL;
ALTER TABLE featured_tags DROP COLUMN tag_id;
ALTER TABLE featured_tags RENAME COLUMN tag_id_new TO tag_id;
ALTER TABLE featured_tags ADD CONSTRAINT featured_tags_account_id_tag_id_key UNIQUE (account_id, tag_id);

-- statuses_tags: composite PK (status_id, tag_id)
ALTER TABLE statuses_tags DROP CONSTRAINT status_tags_pkey;
DROP INDEX IF EXISTS statuses_tags_by_tag;
ALTER TABLE statuses_tags ADD COLUMN tag_id_new BIGINT;
UPDATE statuses_tags st SET tag_id_new = r.new_id FROM tags_remap r WHERE r.old_id = st.tag_id;
ALTER TABLE statuses_tags ALTER COLUMN tag_id_new SET NOT NULL;
ALTER TABLE statuses_tags DROP COLUMN tag_id;
ALTER TABLE statuses_tags RENAME COLUMN tag_id_new TO tag_id;
ALTER TABLE statuses_tags ADD PRIMARY KEY (status_id, tag_id);
CREATE INDEX statuses_tags_by_tag ON statuses_tags (tag_id);

-- tag_follows
DROP INDEX IF EXISTS tag_follows_account_id_tag_id_key;
ALTER TABLE tag_follows ADD COLUMN tag_id_new BIGINT;
UPDATE tag_follows tf SET tag_id_new = r.new_id FROM tags_remap r WHERE r.old_id = tf.tag_id;
ALTER TABLE tag_follows ALTER COLUMN tag_id_new SET NOT NULL;
ALTER TABLE tag_follows DROP COLUMN tag_id;
ALTER TABLE tag_follows RENAME COLUMN tag_id_new TO tag_id;
ALTER TABLE tag_follows ADD CONSTRAINT tag_follows_account_id_tag_id_key UNIQUE (account_id, tag_id);

-- tag_trends
DROP INDEX IF EXISTS tag_trends_tag_id_language_key;
ALTER TABLE tag_trends ADD COLUMN tag_id_new BIGINT;
UPDATE tag_trends tt SET tag_id_new = r.new_id FROM tags_remap r WHERE r.old_id = tt.tag_id;
ALTER TABLE tag_trends ALTER COLUMN tag_id_new SET NOT NULL;
ALTER TABLE tag_trends DROP COLUMN tag_id;
ALTER TABLE tag_trends RENAME COLUMN tag_id_new TO tag_id;
ALTER TABLE tag_trends ADD CONSTRAINT tag_trends_tag_id_language_key UNIQUE (tag_id, language);

-- Finalize tags
ALTER TABLE tags DROP CONSTRAINT tags_pkey;
ALTER TABLE tags DROP COLUMN id;
ALTER TABLE tags RENAME COLUMN new_id TO id;
ALTER TABLE tags ADD PRIMARY KEY (id);
CREATE SEQUENCE tags_id_seq;
SELECT setval('tags_id_seq', (SELECT COALESCE(MAX(id), 1) FROM tags));
ALTER TABLE tags ALTER COLUMN id SET DEFAULT nextval('tags_id_seq');
ALTER SEQUENCE tags_id_seq OWNED BY tags.id;

-- ── Phase 5: polls → poll_votes.poll_id ──────────────────────────────────────

CREATE TEMP TABLE polls_remap AS
    SELECT id AS old_id, ROW_NUMBER() OVER (ORDER BY created_at, id::text) AS new_id
    FROM polls;

ALTER TABLE polls ADD COLUMN new_id BIGINT;
UPDATE polls p SET new_id = r.new_id FROM polls_remap r WHERE r.old_id = p.id;
ALTER TABLE polls ALTER COLUMN new_id SET NOT NULL;

-- poll_votes.poll_id is NOT NULL
DROP INDEX IF EXISTS poll_votes_poll_id_account_id_choice_key;
ALTER TABLE poll_votes ADD COLUMN poll_id_new BIGINT;
UPDATE poll_votes pv SET poll_id_new = r.new_id FROM polls_remap r WHERE r.old_id = pv.poll_id;
ALTER TABLE poll_votes ALTER COLUMN poll_id_new SET NOT NULL;
ALTER TABLE poll_votes DROP COLUMN poll_id;
ALTER TABLE poll_votes RENAME COLUMN poll_id_new TO poll_id;
ALTER TABLE poll_votes ADD CONSTRAINT poll_votes_poll_id_account_id_choice_key
    UNIQUE (poll_id, account_id, choice);

ALTER TABLE polls DROP CONSTRAINT polls_pkey;
ALTER TABLE polls DROP COLUMN id;
ALTER TABLE polls RENAME COLUMN new_id TO id;
ALTER TABLE polls ADD PRIMARY KEY (id);
CREATE SEQUENCE polls_id_seq;
SELECT setval('polls_id_seq', (SELECT COALESCE(MAX(id), 1) FROM polls));
ALTER TABLE polls ALTER COLUMN id SET DEFAULT nextval('polls_id_seq');
ALTER SEQUENCE polls_id_seq OWNED BY polls.id;

-- ── Phase 6: users + all user_id FK columns ──────────────────────────────────

CREATE TEMP TABLE users_remap AS
    SELECT id AS old_id, ROW_NUMBER() OVER (ORDER BY created_at, id::text) AS new_id
    FROM users;

ALTER TABLE users ADD COLUMN new_id BIGINT;
UPDATE users u SET new_id = r.new_id FROM users_remap r WHERE r.old_id = u.id;
ALTER TABLE users ALTER COLUMN new_id SET NOT NULL;

-- Drop indexes on user_id FK columns before dropping those columns
DROP INDEX IF EXISTS index_identities_on_user_id;
DROP INDEX IF EXISTS index_login_activities_on_user_id;
DROP INDEX IF EXISTS index_markers_on_user_id_and_timeline;
DROP INDEX IF EXISTS index_oauth_access_tokens_on_resource_owner_id;
DROP INDEX IF EXISTS index_session_activations_on_user_id;
DROP INDEX IF EXISTS index_user_invite_requests_on_user_id;
DROP INDEX IF EXISTS index_web_push_subscriptions_on_user_id;
DROP INDEX IF EXISTS web_settings_user_id_key;
DROP INDEX IF EXISTS index_webauthn_credentials_on_user_id_and_nickname;
DROP INDEX IF EXISTS index_invites_on_user_id;

-- backups.user_id (nullable)
ALTER TABLE backups ADD COLUMN user_id_new BIGINT;
UPDATE backups b SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = b.user_id;
ALTER TABLE backups DROP COLUMN user_id;
ALTER TABLE backups RENAME COLUMN user_id_new TO user_id;

-- identities.user_id (nullable)
ALTER TABLE identities ADD COLUMN user_id_new BIGINT;
UPDATE identities i SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = i.user_id;
ALTER TABLE identities DROP COLUMN user_id;
ALTER TABLE identities RENAME COLUMN user_id_new TO user_id;
CREATE INDEX index_identities_on_user_id ON identities (user_id);

-- instance_user_sessions.user_id (NOT NULL)
ALTER TABLE instance_user_sessions ADD COLUMN user_id_new BIGINT;
UPDATE instance_user_sessions s SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = s.user_id;
ALTER TABLE instance_user_sessions ALTER COLUMN user_id_new SET NOT NULL;
ALTER TABLE instance_user_sessions DROP COLUMN user_id;
ALTER TABLE instance_user_sessions RENAME COLUMN user_id_new TO user_id;

-- invites.user_id (nullable)
ALTER TABLE invites ADD COLUMN user_id_new BIGINT;
UPDATE invites i SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = i.user_id;
ALTER TABLE invites DROP COLUMN user_id;
ALTER TABLE invites RENAME COLUMN user_id_new TO user_id;
CREATE INDEX index_invites_on_user_id ON invites (user_id) WHERE user_id IS NOT NULL;

-- login_activities.user_id (NOT NULL)
ALTER TABLE login_activities ADD COLUMN user_id_new BIGINT;
UPDATE login_activities la SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = la.user_id;
ALTER TABLE login_activities ALTER COLUMN user_id_new SET NOT NULL;
ALTER TABLE login_activities DROP COLUMN user_id;
ALTER TABLE login_activities RENAME COLUMN user_id_new TO user_id;
CREATE INDEX index_login_activities_on_user_id ON login_activities (user_id);

-- markers.user_id (nullable)
ALTER TABLE markers ADD COLUMN user_id_new BIGINT;
UPDATE markers m SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = m.user_id;
ALTER TABLE markers DROP COLUMN user_id;
ALTER TABLE markers RENAME COLUMN user_id_new TO user_id;
CREATE INDEX index_markers_on_user_id_and_timeline ON markers (user_id, timeline)
    WHERE user_id IS NOT NULL;

-- oauth_access_tokens.resource_owner_id (nullable)
ALTER TABLE oauth_access_tokens ADD COLUMN resource_owner_id_new BIGINT;
UPDATE oauth_access_tokens t SET resource_owner_id_new = r.new_id
    FROM users_remap r WHERE r.old_id = t.resource_owner_id;
ALTER TABLE oauth_access_tokens DROP COLUMN resource_owner_id;
ALTER TABLE oauth_access_tokens RENAME COLUMN resource_owner_id_new TO resource_owner_id;
CREATE INDEX index_oauth_access_tokens_on_resource_owner_id
    ON oauth_access_tokens (resource_owner_id) WHERE resource_owner_id IS NOT NULL;

-- session_activations.user_id (NOT NULL)
ALTER TABLE session_activations ADD COLUMN user_id_new BIGINT;
UPDATE session_activations sa SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = sa.user_id;
ALTER TABLE session_activations ALTER COLUMN user_id_new SET NOT NULL;
ALTER TABLE session_activations DROP COLUMN user_id;
ALTER TABLE session_activations RENAME COLUMN user_id_new TO user_id;
CREATE INDEX index_session_activations_on_user_id ON session_activations (user_id);

-- user_invite_requests.user_id (nullable)
ALTER TABLE user_invite_requests ADD COLUMN user_id_new BIGINT;
UPDATE user_invite_requests ui SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = ui.user_id;
ALTER TABLE user_invite_requests DROP COLUMN user_id;
ALTER TABLE user_invite_requests RENAME COLUMN user_id_new TO user_id;
CREATE INDEX index_user_invite_requests_on_user_id ON user_invite_requests (user_id);

-- user_ips.user_id (no FK constraint, just type)
ALTER TABLE user_ips ADD COLUMN user_id_new BIGINT;
UPDATE user_ips ui SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = ui.user_id;
ALTER TABLE user_ips DROP COLUMN user_id;
ALTER TABLE user_ips RENAME COLUMN user_id_new TO user_id;

-- web_push_subscriptions.user_id (nullable)
ALTER TABLE web_push_subscriptions ADD COLUMN user_id_new BIGINT;
UPDATE web_push_subscriptions w SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = w.user_id;
ALTER TABLE web_push_subscriptions DROP COLUMN user_id;
ALTER TABLE web_push_subscriptions RENAME COLUMN user_id_new TO user_id;
CREATE INDEX index_web_push_subscriptions_on_user_id
    ON web_push_subscriptions (user_id) WHERE user_id IS NOT NULL;

-- web_settings.user_id
ALTER TABLE web_settings ADD COLUMN user_id_new BIGINT;
UPDATE web_settings ws SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = ws.user_id;
ALTER TABLE web_settings DROP COLUMN user_id;
ALTER TABLE web_settings RENAME COLUMN user_id_new TO user_id;
ALTER TABLE web_settings ADD CONSTRAINT web_settings_user_id_key UNIQUE (user_id);

-- webauthn_credentials.user_id
ALTER TABLE webauthn_credentials ADD COLUMN user_id_new BIGINT;
UPDATE webauthn_credentials wc SET user_id_new = r.new_id FROM users_remap r WHERE r.old_id = wc.user_id;
ALTER TABLE webauthn_credentials DROP COLUMN user_id;
ALTER TABLE webauthn_credentials RENAME COLUMN user_id_new TO user_id;
ALTER TABLE webauthn_credentials ADD CONSTRAINT index_webauthn_credentials_on_user_id_and_nickname
    UNIQUE (user_id, nickname);

-- Finalize users
ALTER TABLE users DROP CONSTRAINT users_pkey;
ALTER TABLE users DROP COLUMN id;
ALTER TABLE users RENAME COLUMN new_id TO id;
ALTER TABLE users ADD PRIMARY KEY (id);
CREATE SEQUENCE users_id_seq;
SELECT setval('users_id_seq', (SELECT COALESCE(MAX(id), 1) FROM users));
ALTER TABLE users ALTER COLUMN id SET DEFAULT nextval('users_id_seq');
ALTER SEQUENCE users_id_seq OWNED BY users.id;

-- ── Phase 7: invites → users.invite_id, pending_signups.invite_id ────────────

CREATE TEMP TABLE invites_remap AS
    SELECT id AS old_id, ROW_NUMBER() OVER (ORDER BY created_at, id::text) AS new_id
    FROM invites;

ALTER TABLE invites ADD COLUMN new_id BIGINT;
UPDATE invites i SET new_id = r.new_id FROM invites_remap r WHERE r.old_id = i.id;
ALTER TABLE invites ALTER COLUMN new_id SET NOT NULL;

-- users.invite_id (nullable)
DROP INDEX IF EXISTS users_by_invite;
ALTER TABLE users ADD COLUMN invite_id_new BIGINT;
UPDATE users u SET invite_id_new = r.new_id FROM invites_remap r WHERE r.old_id = u.invite_id;
ALTER TABLE users DROP COLUMN invite_id;
ALTER TABLE users RENAME COLUMN invite_id_new TO invite_id;
CREATE INDEX users_by_invite ON users (invite_id) WHERE invite_id IS NOT NULL;

-- pending_signups.invite_id (nullable)
ALTER TABLE pending_signups ADD COLUMN invite_id_new BIGINT;
UPDATE pending_signups ps SET invite_id_new = r.new_id FROM invites_remap r WHERE r.old_id = ps.invite_id;
ALTER TABLE pending_signups DROP COLUMN invite_id;
ALTER TABLE pending_signups RENAME COLUMN invite_id_new TO invite_id;

ALTER TABLE invites DROP CONSTRAINT invites_pkey;
ALTER TABLE invites DROP COLUMN id;
ALTER TABLE invites RENAME COLUMN new_id TO id;
ALTER TABLE invites ADD PRIMARY KEY (id);
CREATE SEQUENCE invites_id_seq;
SELECT setval('invites_id_seq', (SELECT COALESCE(MAX(id), 1) FROM invites));
ALTER TABLE invites ALTER COLUMN id SET DEFAULT nextval('invites_id_seq');
ALTER SEQUENCE invites_id_seq OWNED BY invites.id;

-- ── Phase 8: oauth_access_tokens → session_activations, web_push_subscriptions

CREATE TEMP TABLE oat_remap AS
    SELECT id AS old_id, ROW_NUMBER() OVER (ORDER BY created_at, id::text) AS new_id
    FROM oauth_access_tokens;

ALTER TABLE oauth_access_tokens ADD COLUMN new_id BIGINT;
UPDATE oauth_access_tokens t SET new_id = r.new_id FROM oat_remap r WHERE r.old_id = t.id;
ALTER TABLE oauth_access_tokens ALTER COLUMN new_id SET NOT NULL;

-- session_activations.access_token_id (nullable)
DROP INDEX IF EXISTS index_session_activations_on_access_token_id;
ALTER TABLE session_activations ADD COLUMN access_token_id_new BIGINT;
UPDATE session_activations sa SET access_token_id_new = r.new_id
    FROM oat_remap r WHERE r.old_id = sa.access_token_id;
ALTER TABLE session_activations DROP COLUMN access_token_id;
ALTER TABLE session_activations RENAME COLUMN access_token_id_new TO access_token_id;
CREATE INDEX index_session_activations_on_access_token_id
    ON session_activations (access_token_id) WHERE access_token_id IS NOT NULL;

-- web_push_subscriptions.access_token_id (nullable)
DROP INDEX IF EXISTS web_push_subscriptions_access_token_id_key;
ALTER TABLE web_push_subscriptions ADD COLUMN access_token_id_new BIGINT;
UPDATE web_push_subscriptions w SET access_token_id_new = r.new_id
    FROM oat_remap r WHERE r.old_id = w.access_token_id;
ALTER TABLE web_push_subscriptions DROP COLUMN access_token_id;
ALTER TABLE web_push_subscriptions RENAME COLUMN access_token_id_new TO access_token_id;
ALTER TABLE web_push_subscriptions ADD CONSTRAINT web_push_subscriptions_access_token_id_key
    UNIQUE (access_token_id);

ALTER TABLE oauth_access_tokens DROP CONSTRAINT oauth_access_tokens_pkey;
ALTER TABLE oauth_access_tokens DROP COLUMN id;
ALTER TABLE oauth_access_tokens RENAME COLUMN new_id TO id;
ALTER TABLE oauth_access_tokens ADD PRIMARY KEY (id);
CREATE SEQUENCE oauth_access_tokens_id_seq;
SELECT setval('oauth_access_tokens_id_seq', (SELECT COALESCE(MAX(id), 1) FROM oauth_access_tokens));
ALTER TABLE oauth_access_tokens ALTER COLUMN id SET DEFAULT nextval('oauth_access_tokens_id_seq');
ALTER SEQUENCE oauth_access_tokens_id_seq OWNED BY oauth_access_tokens.id;

-- ── Phase 9: Recreate all FK constraints ─────────────────────────────────────

ALTER TABLE list_accounts ADD CONSTRAINT list_accounts_follow_id_fkey
    FOREIGN KEY (follow_id) REFERENCES follows(id) ON DELETE SET NULL;
ALTER TABLE announcement_reactions ADD CONSTRAINT announcement_reactions_custom_emoji_id_fkey
    FOREIGN KEY (custom_emoji_id) REFERENCES custom_emojis(id) ON DELETE CASCADE;
ALTER TABLE accounts_tags ADD CONSTRAINT accounts_tags_tag_id_fkey
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE;
ALTER TABLE featured_tags ADD CONSTRAINT featured_tags_tag_id_fkey
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE;
ALTER TABLE statuses_tags ADD CONSTRAINT statuses_tags_tag_id_fkey
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE;
ALTER TABLE tag_follows ADD CONSTRAINT tag_follows_tag_id_fkey
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE;
ALTER TABLE tag_trends ADD CONSTRAINT tag_trends_tag_id_fkey
    FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE;
ALTER TABLE poll_votes ADD CONSTRAINT poll_votes_poll_id_fkey
    FOREIGN KEY (poll_id) REFERENCES polls(id) ON DELETE CASCADE;
ALTER TABLE users ADD CONSTRAINT users_invite_id_fkey
    FOREIGN KEY (invite_id) REFERENCES invites(id) ON DELETE SET NULL;
ALTER TABLE pending_signups ADD CONSTRAINT pending_signups_invite_id_fkey
    FOREIGN KEY (invite_id) REFERENCES invites(id);
ALTER TABLE session_activations ADD CONSTRAINT session_activations_access_token_id_fkey
    FOREIGN KEY (access_token_id) REFERENCES oauth_access_tokens(id) ON DELETE SET NULL;
ALTER TABLE web_push_subscriptions ADD CONSTRAINT web_push_subscriptions_access_token_id_fkey
    FOREIGN KEY (access_token_id) REFERENCES oauth_access_tokens(id) ON DELETE CASCADE;
ALTER TABLE backups ADD CONSTRAINT backups_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE identities ADD CONSTRAINT identities_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE instance_user_sessions ADD CONSTRAINT instance_user_sessions_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE invites ADD CONSTRAINT invites_user_id_fkey
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE login_activities ADD CONSTRAINT login_activities_user_id_fkey
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
