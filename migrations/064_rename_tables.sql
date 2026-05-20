-- Align eunha table names with Mastodon's canonical schema.
-- See: https://github.com/mastodon/mastodon schema reference.

-- 1. admin_ip_blocks → ip_blocks (columns are already identical)
ALTER TABLE admin_ip_blocks RENAME TO ip_blocks;
ALTER INDEX admin_ip_blocks_pkey RENAME TO ip_blocks_pkey;

-- 2. status_tags → statuses_tags
ALTER TABLE status_tags RENAME TO statuses_tags;
ALTER TABLE statuses_tags RENAME CONSTRAINT status_tags_status_id_fkey TO statuses_tags_status_id_fkey;
ALTER TABLE statuses_tags RENAME CONSTRAINT status_tags_tag_id_fkey TO statuses_tags_tag_id_fkey;
ALTER INDEX status_tags_by_tag RENAME TO statuses_tags_by_tag;

-- 3. user_domain_blocks → account_domain_blocks
ALTER TABLE user_domain_blocks RENAME TO account_domain_blocks;
ALTER TABLE account_domain_blocks RENAME CONSTRAINT user_domain_blocks_pkey TO account_domain_blocks_pkey;
ALTER TABLE account_domain_blocks RENAME CONSTRAINT user_domain_blocks_account_id_fkey TO account_domain_blocks_account_id_fkey;
ALTER TABLE account_domain_blocks ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();
ALTER TABLE account_domain_blocks ADD UNIQUE (account_id, domain);
CREATE INDEX index_account_domain_blocks_on_account_id_and_domain ON account_domain_blocks(account_id, domain);

-- 4. suggestion_dismissals → follow_recommendation_mutes
ALTER TABLE suggestion_dismissals RENAME TO follow_recommendation_mutes;
ALTER TABLE follow_recommendation_mutes RENAME CONSTRAINT suggestion_dismissals_pkey TO follow_recommendation_mutes_pkey;
ALTER TABLE follow_recommendation_mutes RENAME CONSTRAINT suggestion_dismissals_account_id_fkey TO follow_recommendation_mutes_account_id_fkey;
ALTER TABLE follow_recommendation_mutes RENAME CONSTRAINT suggestion_dismissals_target_account_id_fkey TO follow_recommendation_mutes_target_account_id_fkey;
ALTER TABLE follow_recommendation_mutes ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

-- follow_recommendation_suppressions mirrors follow_recommendation_mutes but for suppressions
-- (Mastodon has both; create alias table for compat)
CREATE TABLE IF NOT EXISTS follow_recommendation_suppressions (
    id         BIGSERIAL PRIMARY KEY,
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id)
);

-- 5. admin_email_domain_blocks → email_domain_blocks
ALTER TABLE admin_email_domain_blocks RENAME TO email_domain_blocks;
ALTER TABLE email_domain_blocks RENAME CONSTRAINT admin_email_domain_blocks_pkey TO email_domain_blocks_pkey;
ALTER TABLE email_domain_blocks ADD COLUMN allow_with_approval BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE email_domain_blocks ADD COLUMN parent_id BIGINT REFERENCES email_domain_blocks(id) ON DELETE SET NULL;

-- 6. status_preview_cards → preview_cards_statuses (rename card_id → preview_card_id, add url)
ALTER TABLE status_preview_cards RENAME COLUMN card_id TO preview_card_id;
ALTER TABLE status_preview_cards ADD COLUMN url TEXT NOT NULL DEFAULT '';
ALTER TABLE status_preview_cards RENAME TO preview_cards_statuses;
ALTER TABLE preview_cards_statuses RENAME CONSTRAINT status_preview_cards_pkey TO preview_cards_statuses_pkey;
ALTER TABLE preview_cards_statuses RENAME CONSTRAINT status_preview_cards_status_id_fkey TO preview_cards_statuses_status_id_fkey;
ALTER TABLE preview_cards_statuses RENAME CONSTRAINT status_preview_cards_card_id_fkey TO preview_cards_statuses_preview_card_id_fkey;

-- 7. announcement_dismissals → announcement_mutes
-- Add id PK + timestamps; restructure from composite PK
ALTER TABLE announcement_dismissals ADD COLUMN id BIGSERIAL;
ALTER TABLE announcement_dismissals ADD COLUMN created_at TIMESTAMPTZ NOT NULL DEFAULT now();
ALTER TABLE announcement_dismissals ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();
ALTER TABLE announcement_dismissals DROP CONSTRAINT announcement_dismissals_pkey;
ALTER TABLE announcement_dismissals ADD PRIMARY KEY (id);
ALTER TABLE announcement_dismissals ADD UNIQUE (account_id, announcement_id);
ALTER TABLE announcement_dismissals RENAME TO announcement_mutes;
ALTER TABLE announcement_mutes RENAME CONSTRAINT announcement_dismissals_account_id_fkey TO announcement_mutes_account_id_fkey;
ALTER TABLE announcement_mutes RENAME CONSTRAINT announcement_dismissals_announcement_id_fkey TO announcement_mutes_announcement_id_fkey;
CREATE INDEX index_announcement_mutes_on_account_id_and_announcement_id ON announcement_mutes(account_id, announcement_id);

-- 8. oauth_authorization_codes → oauth_access_grants
-- Rename code → token; add revoked_at, expires_in, resource_owner_id
ALTER TABLE oauth_authorization_codes RENAME COLUMN code TO token;
ALTER TABLE oauth_authorization_codes ADD COLUMN revoked_at TIMESTAMPTZ;
ALTER TABLE oauth_authorization_codes ADD COLUMN expires_in INTEGER;
-- Compute expires_in (seconds) from expires_at and created_at for existing rows
UPDATE oauth_authorization_codes
    SET expires_in = EXTRACT(EPOCH FROM (expires_at - created_at))::INTEGER
    WHERE expires_at IS NOT NULL AND created_at IS NOT NULL;
-- resource_owner_id mirrors account_id; kept nullable since eunha users.id is UUID not BIGINT
ALTER TABLE oauth_authorization_codes ADD COLUMN resource_owner_id BIGINT;
UPDATE oauth_authorization_codes SET resource_owner_id = account_id WHERE account_id IS NOT NULL;
ALTER TABLE oauth_authorization_codes RENAME TO oauth_access_grants;
ALTER TABLE oauth_access_grants RENAME CONSTRAINT oauth_authorization_codes_pkey TO oauth_access_grants_pkey;
ALTER TABLE oauth_access_grants RENAME CONSTRAINT oauth_authorization_codes_application_id_fkey TO oauth_access_grants_application_id_fkey;
ALTER TABLE oauth_access_grants RENAME CONSTRAINT oauth_authorization_codes_account_id_fkey TO oauth_access_grants_account_id_fkey;
ALTER INDEX IF EXISTS oauth_authorization_codes_token_key RENAME TO oauth_access_grants_token_key;
