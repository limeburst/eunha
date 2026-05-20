-- Sync eunha schema to Mastodon's canonical schema.
-- Reference: mastodon_src database (local Mastodon instance).
-- Deliberate eunha differences kept: UUIDs for user/invite/etc IDs,
-- TIMESTAMPTZ vs timestamp without time zone, TEXT vs varchar,
-- text enums vs integer enums (Rust enum serialization), multi-tenancy columns.

-- ────────────────────────────────────────────────────────────────────────────
-- 1. RENAME annual_reports → generated_annual_reports
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE annual_reports RENAME TO generated_annual_reports;
ALTER TABLE generated_annual_reports RENAME CONSTRAINT annual_reports_pkey TO generated_annual_reports_pkey;
ALTER TABLE generated_annual_reports RENAME CONSTRAINT annual_reports_account_id_fkey TO generated_annual_reports_account_id_fkey;
ALTER TABLE generated_annual_reports RENAME CONSTRAINT annual_reports_account_id_year_key TO generated_annual_reports_account_id_year_key;

-- ────────────────────────────────────────────────────────────────────────────
-- 2. CREATE annual_report_statuses_per_account_counts
-- ────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS annual_report_statuses_per_account_counts (
    id              BIGSERIAL PRIMARY KEY,
    year            INTEGER NOT NULL,
    account_id      BIGINT NOT NULL,
    statuses_count  BIGINT NOT NULL,
    UNIQUE (year, account_id)
);

-- ────────────────────────────────────────────────────────────────────────────
-- 3. account_relationship_severance_events — add updated_at
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE account_relationship_severance_events
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ────────────────────────────────────────────────────────────────────────────
-- 4. accounts — Paperclip/ActiveStorage file columns + nullability fixes
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE accounts
    ADD COLUMN IF NOT EXISTS avatar_file_name               TEXT,
    ADD COLUMN IF NOT EXISTS avatar_content_type            TEXT,
    ADD COLUMN IF NOT EXISTS avatar_file_size               INTEGER,
    ADD COLUMN IF NOT EXISTS avatar_updated_at              TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS header_file_name               TEXT,
    ADD COLUMN IF NOT EXISTS header_content_type            TEXT,
    ADD COLUMN IF NOT EXISTS header_file_size               INTEGER,
    ADD COLUMN IF NOT EXISTS header_updated_at              TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS avatar_remote_url              TEXT,
    ADD COLUMN IF NOT EXISTS header_remote_url              TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS avatar_storage_schema_version  INTEGER,
    ADD COLUMN IF NOT EXISTS header_storage_schema_version  INTEGER;

-- Mastodon allows NULL for url and shared_inbox_url
ALTER TABLE accounts
    ALTER COLUMN url            DROP NOT NULL,
    ALTER COLUMN shared_inbox_url DROP NOT NULL;

-- Mastodon allows NULL for fields and also_known_as and attribution_domains and hide_collections
ALTER TABLE accounts
    ALTER COLUMN fields             DROP NOT NULL,
    ALTER COLUMN also_known_as      DROP NOT NULL,
    ALTER COLUMN attribution_domains DROP NOT NULL,
    ALTER COLUMN hide_collections   DROP NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 5. account_warnings — make target_account_id nullable (matches Mastodon)
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE account_warnings
    ALTER COLUMN target_account_id DROP NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 6. announcements — missing columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE announcements
    ADD COLUMN IF NOT EXISTS scheduled_at        TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS status_ids          BIGINT[],
    ADD COLUMN IF NOT EXISTS notification_sent_at TIMESTAMPTZ;

-- published_at should be nullable (mastodon allows null = draft)
ALTER TABLE announcements
    ALTER COLUMN published_at DROP NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 7. backups — Paperclip file columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE backups
    ADD COLUMN IF NOT EXISTS dump_file_name    TEXT,
    ADD COLUMN IF NOT EXISTS dump_content_type TEXT,
    ADD COLUMN IF NOT EXISTS dump_updated_at   TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS dump_file_size    BIGINT;

-- ────────────────────────────────────────────────────────────────────────────
-- 8. conversation_mutes — add conversation_id (Mastodon FK)
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE conversation_mutes
    ADD COLUMN IF NOT EXISTS conversation_id BIGINT REFERENCES conversations(id) ON DELETE CASCADE;
CREATE INDEX IF NOT EXISTS index_conversation_mutes_on_account_id_and_conversation_id
    ON conversation_mutes(account_id, conversation_id) WHERE conversation_id IS NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 9. conversations — missing columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE conversations
    ADD COLUMN IF NOT EXISTS uri               TEXT,
    ADD COLUMN IF NOT EXISTS parent_status_id  BIGINT,
    ADD COLUMN IF NOT EXISTS parent_account_id BIGINT;
CREATE UNIQUE INDEX IF NOT EXISTS index_conversations_on_uri
    ON conversations(uri) WHERE uri IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS index_conversations_on_parent_status_id
    ON conversations(parent_status_id) WHERE parent_status_id IS NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 10. custom_emojis — Paperclip file columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE custom_emojis
    ADD COLUMN IF NOT EXISTS image_file_name               TEXT,
    ADD COLUMN IF NOT EXISTS image_content_type            TEXT,
    ADD COLUMN IF NOT EXISTS image_file_size               INTEGER,
    ADD COLUMN IF NOT EXISTS image_updated_at              TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS image_remote_url              TEXT,
    ADD COLUMN IF NOT EXISTS image_storage_schema_version  INTEGER;

-- ────────────────────────────────────────────────────────────────────────────
-- 11. fasp_backfill_requests — missing columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE fasp_backfill_requests
    ADD COLUMN IF NOT EXISTS category TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS cursor   TEXT;

-- ────────────────────────────────────────────────────────────────────────────
-- 12. fasp_debug_callbacks — missing columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE fasp_debug_callbacks
    ADD COLUMN IF NOT EXISTS ip           TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS request_body TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS updated_at   TIMESTAMPTZ NOT NULL DEFAULT now();

-- ────────────────────────────────────────────────────────────────────────────
-- 13. fasp_follow_recommendations — missing columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE fasp_follow_recommendations
    ADD COLUMN IF NOT EXISTS requesting_account_id  BIGINT REFERENCES accounts(id) ON DELETE CASCADE,
    ADD COLUMN IF NOT EXISTS recommended_account_id BIGINT REFERENCES accounts(id) ON DELETE CASCADE;
CREATE INDEX IF NOT EXISTS index_fasp_follow_recommendations_on_requesting_account_id
    ON fasp_follow_recommendations(requesting_account_id) WHERE requesting_account_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS index_fasp_follow_recommendations_on_recommended_account_id
    ON fasp_follow_recommendations(recommended_account_id) WHERE recommended_account_id IS NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 14. fasp_providers — missing columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE fasp_providers
    ADD COLUMN IF NOT EXISTS provider_public_key_pem  TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS server_private_key_pem   TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS capabilities             JSONB NOT NULL DEFAULT '[]'::JSONB,
    ADD COLUMN IF NOT EXISTS privacy_policy           JSONB,
    ADD COLUMN IF NOT EXISTS contact_email            TEXT,
    ADD COLUMN IF NOT EXISTS fediverse_account        TEXT,
    ADD COLUMN IF NOT EXISTS delivery_last_failed_at  TIMESTAMPTZ;

-- remote_identifier should be NOT NULL in mastodon; make it so with a safe default
UPDATE fasp_providers SET remote_identifier = '' WHERE remote_identifier IS NULL;
ALTER TABLE fasp_providers ALTER COLUMN remote_identifier SET DEFAULT '';
ALTER TABLE fasp_providers ALTER COLUMN remote_identifier SET NOT NULL;

-- Unique index on base_url (matches mastodon)
CREATE UNIQUE INDEX IF NOT EXISTS index_fasp_providers_on_base_url ON fasp_providers(base_url);

-- ────────────────────────────────────────────────────────────────────────────
-- 15. fasp_subscriptions — missing columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE fasp_subscriptions
    ADD COLUMN IF NOT EXISTS subscription_type   TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS max_batch_size      INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS threshold_timeframe INTEGER,
    ADD COLUMN IF NOT EXISTS threshold_shares    INTEGER,
    ADD COLUMN IF NOT EXISTS threshold_likes     INTEGER,
    ADD COLUMN IF NOT EXISTS threshold_replies   INTEGER;

-- ────────────────────────────────────────────────────────────────────────────
-- 16. invites — add user_id (Mastodon FK to users; eunha uses created_by → accounts)
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE invites
    ADD COLUMN IF NOT EXISTS user_id UUID REFERENCES users(id) ON DELETE CASCADE;
CREATE INDEX IF NOT EXISTS index_invites_on_user_id
    ON invites(user_id) WHERE user_id IS NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 17. markers — add user_id (Mastodon FK to users; eunha uses account_id)
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE markers
    ADD COLUMN IF NOT EXISTS user_id UUID REFERENCES users(id) ON DELETE CASCADE;
CREATE INDEX IF NOT EXISTS index_markers_on_user_id_and_timeline
    ON markers(user_id, timeline) WHERE user_id IS NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 18. media_attachments — missing columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE media_attachments
    ADD COLUMN IF NOT EXISTS updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS shortcode                   TEXT,
    ADD COLUMN IF NOT EXISTS type                        INTEGER,
    ADD COLUMN IF NOT EXISTS file_meta                   JSON,
    ADD COLUMN IF NOT EXISTS scheduled_status_id         BIGINT REFERENCES scheduled_statuses(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS processing                  INTEGER,
    ADD COLUMN IF NOT EXISTS file_storage_schema_version INTEGER,
    ADD COLUMN IF NOT EXISTS file_file_name              TEXT,
    ADD COLUMN IF NOT EXISTS file_content_type           TEXT,
    ADD COLUMN IF NOT EXISTS file_file_size              INTEGER,
    ADD COLUMN IF NOT EXISTS file_updated_at             TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS thumbnail_file_name         TEXT,
    ADD COLUMN IF NOT EXISTS thumbnail_content_type      TEXT,
    ADD COLUMN IF NOT EXISTS thumbnail_file_size         INTEGER,
    ADD COLUMN IF NOT EXISTS thumbnail_updated_at        TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS thumbnail_remote_url        TEXT;

-- Populate type from media_type text enum for existing rows
UPDATE media_attachments SET type = CASE media_type
    WHEN 'image'   THEN 0
    WHEN 'gifv'    THEN 1
    WHEN 'video'   THEN 2
    WHEN 'audio'   THEN 3
    ELSE 4
END WHERE type IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS index_media_attachments_on_shortcode
    ON media_attachments(shortcode) WHERE shortcode IS NOT NULL;
CREATE INDEX IF NOT EXISTS index_media_attachments_on_scheduled_status_id
    ON media_attachments(scheduled_status_id) WHERE scheduled_status_id IS NOT NULL;

-- account_id should be nullable in mastodon (uploaded before status exists)
ALTER TABLE media_attachments ALTER COLUMN account_id DROP NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 19. notifications — missing columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE notifications
    ADD COLUMN IF NOT EXISTS activity_id   BIGINT,
    ADD COLUMN IF NOT EXISTS activity_type TEXT,
    ADD COLUMN IF NOT EXISTS updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS type          TEXT;

-- Populate type from notification_type for existing rows
UPDATE notifications SET type = notification_type WHERE type IS NULL;

CREATE INDEX IF NOT EXISTS index_notifications_on_activity_id_and_activity_type
    ON notifications(activity_id, activity_type)
    WHERE activity_id IS NOT NULL AND activity_type IS NOT NULL;
CREATE INDEX IF NOT EXISTS index_notifications_on_from_account_id
    ON notifications(from_account_id);

-- ────────────────────────────────────────────────────────────────────────────
-- 20. oauth_access_tokens — missing columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE oauth_access_tokens
    ADD COLUMN IF NOT EXISTS expires_in        INTEGER,
    ADD COLUMN IF NOT EXISTS resource_owner_id UUID REFERENCES users(id) ON DELETE CASCADE,
    ADD COLUMN IF NOT EXISTS last_used_at      TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS last_used_ip      INET;

CREATE INDEX IF NOT EXISTS index_oauth_access_tokens_on_resource_owner_id
    ON oauth_access_tokens(resource_owner_id) WHERE resource_owner_id IS NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 21. oauth_applications — add Mastodon-compatible column aliases
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE oauth_applications
    ADD COLUMN IF NOT EXISTS uid          TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS secret       TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS redirect_uri TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS owner_type   TEXT,
    ADD COLUMN IF NOT EXISTS owner_id     BIGINT;

-- Populate from eunha's existing columns
UPDATE oauth_applications SET uid          = client_id     WHERE uid = '';
UPDATE oauth_applications SET secret       = client_secret WHERE secret = '';
UPDATE oauth_applications SET redirect_uri = redirect_uris WHERE redirect_uri = '';

CREATE UNIQUE INDEX IF NOT EXISTS index_oauth_applications_on_uid
    ON oauth_applications(uid) WHERE uid <> '';
CREATE INDEX IF NOT EXISTS index_oauth_applications_on_owner_id_and_owner_type
    ON oauth_applications(owner_id, owner_type) WHERE owner_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS index_oauth_applications_on_superapp
    ON oauth_applications(superapp) WHERE superapp = true;

-- ────────────────────────────────────────────────────────────────────────────
-- 22. preview_card_providers — Paperclip file columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE preview_card_providers
    ADD COLUMN IF NOT EXISTS icon_file_name    TEXT,
    ADD COLUMN IF NOT EXISTS icon_content_type TEXT,
    ADD COLUMN IF NOT EXISTS icon_file_size    BIGINT,
    ADD COLUMN IF NOT EXISTS icon_updated_at   TIMESTAMPTZ;

-- ────────────────────────────────────────────────────────────────────────────
-- 23. preview_cards — Paperclip file columns + image_description
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE preview_cards
    ADD COLUMN IF NOT EXISTS image_file_name               TEXT,
    ADD COLUMN IF NOT EXISTS image_content_type            TEXT,
    ADD COLUMN IF NOT EXISTS image_file_size               INTEGER,
    ADD COLUMN IF NOT EXISTS image_updated_at              TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS image_storage_schema_version  INTEGER,
    ADD COLUMN IF NOT EXISTS image_description             TEXT NOT NULL DEFAULT '';

-- type should be NOT NULL in mastodon (default 0 = link)
UPDATE preview_cards SET type = 0 WHERE type IS NULL;
ALTER TABLE preview_cards ALTER COLUMN type SET DEFAULT 0;
ALTER TABLE preview_cards ALTER COLUMN type SET NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 24. quotes — add legacy flag
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE quotes
    ADD COLUMN IF NOT EXISTS legacy BOOLEAN NOT NULL DEFAULT false;

-- ────────────────────────────────────────────────────────────────────────────
-- 25. rule_translations — add language
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE rule_translations
    ADD COLUMN IF NOT EXISTS language TEXT NOT NULL DEFAULT '';

-- ────────────────────────────────────────────────────────────────────────────
-- 26. session_activations — add web_push_subscription_id
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE session_activations
    ADD COLUMN IF NOT EXISTS web_push_subscription_id BIGINT REFERENCES web_push_subscriptions(id) ON DELETE SET NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 27. site_uploads — Paperclip file columns + blurhash
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE site_uploads
    ADD COLUMN IF NOT EXISTS file_file_name    TEXT,
    ADD COLUMN IF NOT EXISTS file_content_type TEXT,
    ADD COLUMN IF NOT EXISTS file_file_size    INTEGER,
    ADD COLUMN IF NOT EXISTS file_updated_at   TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS blurhash          TEXT;

-- ────────────────────────────────────────────────────────────────────────────
-- 28. status_pins — add updated_at
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE status_pins
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- ────────────────────────────────────────────────────────────────────────────
-- 29. terms_of_services — add effective_date
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE terms_of_services
    ADD COLUMN IF NOT EXISTS effective_date DATE;

-- ────────────────────────────────────────────────────────────────────────────
-- 30. username_blocks — rename exact_match → exact, add missing columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE username_blocks RENAME COLUMN exact_match TO exact;
ALTER TABLE username_blocks
    ADD COLUMN IF NOT EXISTS normalized_username TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS allow_with_approval BOOLEAN NOT NULL DEFAULT false;
CREATE INDEX IF NOT EXISTS index_username_blocks_on_normalized_username
    ON username_blocks(normalized_username);
CREATE UNIQUE INDEX IF NOT EXISTS index_username_blocks_on_username_lower_btree
    ON username_blocks(lower(username));

-- ────────────────────────────────────────────────────────────────────────────
-- 31. users — many missing Mastodon columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE users
    -- Auth/login fields
    ADD COLUMN IF NOT EXISTS encrypted_password       TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS reset_password_token     TEXT,
    ADD COLUMN IF NOT EXISTS reset_password_sent_at   TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS sign_in_count            INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS current_sign_in_at       TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS last_sign_in_at          TIMESTAMPTZ,
    -- Email confirmation
    ADD COLUMN IF NOT EXISTS confirmation_sent_at     TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS unconfirmed_email        TEXT,
    -- Preferences
    ADD COLUMN IF NOT EXISTS locale                   TEXT,
    ADD COLUMN IF NOT EXISTS chosen_languages         TEXT[],
    ADD COLUMN IF NOT EXISTS time_zone                TEXT,
    ADD COLUMN IF NOT EXISTS settings                 TEXT,
    -- TOTP/OTP
    ADD COLUMN IF NOT EXISTS consumed_timestep        INTEGER,
    ADD COLUMN IF NOT EXISTS otp_required_for_login   BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS otp_backup_codes         TEXT[],
    ADD COLUMN IF NOT EXISTS otp_secret               TEXT,
    -- Sign-in token (email MFA)
    ADD COLUMN IF NOT EXISTS sign_in_token            TEXT,
    ADD COLUMN IF NOT EXISTS sign_in_token_sent_at    TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS skip_sign_in_token       BOOLEAN,
    -- WebAuthn
    ADD COLUMN IF NOT EXISTS webauthn_id              TEXT,
    -- Admin/moderation
    ADD COLUMN IF NOT EXISTS last_emailed_at          TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS disabled                 BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS approved                 BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS sign_up_ip               INET,
    ADD COLUMN IF NOT EXISTS created_by_application_id BIGINT REFERENCES oauth_applications(id) ON DELETE SET NULL,
    -- ToS
    ADD COLUMN IF NOT EXISTS age_verified_at          TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS require_tos_interstitial BOOLEAN NOT NULL DEFAULT false;

-- Populate compat columns from eunha's equivalent columns
UPDATE users SET encrypted_password     = password_hash          WHERE encrypted_password = '' AND password_hash IS NOT NULL;
UPDATE users SET reset_password_token   = password_reset_token   WHERE reset_password_token IS NULL AND password_reset_token IS NOT NULL;
UPDATE users SET reset_password_sent_at = password_reset_sent_at WHERE reset_password_sent_at IS NULL AND password_reset_sent_at IS NOT NULL;
UPDATE users SET locale                 = default_language        WHERE locale IS NULL AND default_language IS NOT NULL;

-- approved: eunha uses approved_at timestamp; Mastodon uses approved boolean.
-- Set approved = (approved_at IS NOT NULL OR approved_at IS NOT REQUIRED)
UPDATE users SET approved = (approved_at IS NOT NULL) WHERE approved IS NULL OR approved = true;

CREATE INDEX IF NOT EXISTS index_users_on_confirmation_token
    ON users(confirmation_token) WHERE confirmation_token IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS index_users_on_reset_password_token
    ON users(reset_password_token) WHERE reset_password_token IS NOT NULL;
CREATE INDEX IF NOT EXISTS index_users_on_unconfirmed_email
    ON users(unconfirmed_email) WHERE unconfirmed_email IS NOT NULL;
CREATE INDEX IF NOT EXISTS index_users_on_created_by_application_id
    ON users(created_by_application_id) WHERE created_by_application_id IS NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 32. web_push_subscriptions — add Mastodon-compatible columns
-- ────────────────────────────────────────────────────────────────────────────
ALTER TABLE web_push_subscriptions
    ADD COLUMN IF NOT EXISTS key_p256dh TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS key_auth   TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS data       JSON,
    ADD COLUMN IF NOT EXISTS user_id    UUID REFERENCES users(id) ON DELETE CASCADE,
    ADD COLUMN IF NOT EXISTS standard   BOOLEAN NOT NULL DEFAULT false;

-- Populate from eunha's existing p256dh/auth columns
UPDATE web_push_subscriptions SET key_p256dh = p256dh WHERE key_p256dh = '';
UPDATE web_push_subscriptions SET key_auth   = auth   WHERE key_auth   = '';

CREATE INDEX IF NOT EXISTS index_web_push_subscriptions_on_user_id
    ON web_push_subscriptions(user_id) WHERE user_id IS NOT NULL;

-- ────────────────────────────────────────────────────────────────────────────
-- 33. CREATE user_ips view (Mastodon compatibility)
-- ────────────────────────────────────────────────────────────────────────────
CREATE OR REPLACE VIEW user_ips AS
SELECT user_id, ip, max(used_at) AS used_at
FROM (
    SELECT u.id AS user_id, u.sign_up_ip AS ip, u.created_at AS used_at
    FROM users u
    WHERE u.sign_up_ip IS NOT NULL
    UNION ALL
    SELECT sa.user_id, sa.ip, sa.updated_at
    FROM session_activations sa
    WHERE sa.ip IS NOT NULL
    UNION ALL
    SELECT la.user_id, la.ip, la.created_at
    FROM login_activities la
    WHERE la.ip IS NOT NULL AND la.success = true
) t
GROUP BY user_id, ip;
