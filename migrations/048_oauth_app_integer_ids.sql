-- Replace UUID primary key on oauth_applications with a BIGSERIAL integer.
-- All FK columns in referencing tables are updated to BIGINT to match.

-- 1. Add new integer id column (BIGSERIAL auto-assigns sequential IDs to existing rows)
ALTER TABLE oauth_applications ADD COLUMN id_new BIGSERIAL;

-- 2. Add integer FK columns in referencing tables
ALTER TABLE oauth_authorization_codes ADD COLUMN application_id_new BIGINT;
ALTER TABLE oauth_access_tokens       ADD COLUMN application_id_new BIGINT;
ALTER TABLE statuses                  ADD COLUMN application_id_new BIGINT;
ALTER TABLE pending_signups           ADD COLUMN app_id_new         BIGINT;

-- 3. Populate new FK columns by joining on the old UUID
UPDATE oauth_authorization_codes c SET application_id_new = a.id_new
  FROM oauth_applications a WHERE c.application_id = a.id;

UPDATE oauth_access_tokens t SET application_id_new = a.id_new
  FROM oauth_applications a WHERE t.application_id = a.id;

UPDATE statuses s SET application_id_new = a.id_new
  FROM oauth_applications a WHERE s.application_id = a.id;

UPDATE pending_signups p SET app_id_new = a.id_new
  FROM oauth_applications a WHERE p.app_id = a.id;

-- 4. Clean up orphan codes before restoring NOT NULL (shouldn't exist, but be safe)
DELETE FROM oauth_authorization_codes WHERE application_id_new IS NULL;

-- 5. Drop old FK constraints
ALTER TABLE oauth_authorization_codes DROP CONSTRAINT oauth_authorization_codes_application_id_fkey;
ALTER TABLE oauth_access_tokens       DROP CONSTRAINT oauth_access_tokens_application_id_fkey;
ALTER TABLE statuses                  DROP CONSTRAINT statuses_application_id_fkey;
ALTER TABLE pending_signups           DROP CONSTRAINT pending_signups_app_id_fkey;

-- 6. Drop old UUID columns
ALTER TABLE oauth_authorization_codes DROP COLUMN application_id;
ALTER TABLE oauth_access_tokens       DROP COLUMN application_id;
ALTER TABLE statuses                  DROP COLUMN application_id;
ALTER TABLE pending_signups           DROP COLUMN app_id;

-- 7. Rename new integer columns to their canonical names
ALTER TABLE oauth_authorization_codes RENAME COLUMN application_id_new TO application_id;
ALTER TABLE oauth_access_tokens       RENAME COLUMN application_id_new TO application_id;
ALTER TABLE statuses                  RENAME COLUMN application_id_new TO application_id;
ALTER TABLE pending_signups           RENAME COLUMN app_id_new         TO app_id;

-- 8. Restore NOT NULL on oauth_authorization_codes.application_id
ALTER TABLE oauth_authorization_codes ALTER COLUMN application_id SET NOT NULL;

-- 9. Swap the primary key on oauth_applications
ALTER TABLE oauth_applications DROP CONSTRAINT oauth_applications_pkey;
ALTER TABLE oauth_applications DROP COLUMN id;
ALTER TABLE oauth_applications RENAME COLUMN id_new TO id;
ALTER TABLE oauth_applications ADD PRIMARY KEY (id);
ALTER SEQUENCE oauth_applications_id_new_seq RENAME TO oauth_applications_id_seq;

-- 10. Re-add FK constraints with matching types
ALTER TABLE oauth_authorization_codes
  ADD CONSTRAINT oauth_authorization_codes_application_id_fkey
  FOREIGN KEY (application_id) REFERENCES oauth_applications(id) ON DELETE CASCADE;

ALTER TABLE oauth_access_tokens
  ADD CONSTRAINT oauth_access_tokens_application_id_fkey
  FOREIGN KEY (application_id) REFERENCES oauth_applications(id) ON DELETE CASCADE;

ALTER TABLE statuses
  ADD CONSTRAINT statuses_application_id_fkey
  FOREIGN KEY (application_id) REFERENCES oauth_applications(id) ON DELETE SET NULL;

ALTER TABLE pending_signups
  ADD CONSTRAINT pending_signups_app_id_fkey
  FOREIGN KEY (app_id) REFERENCES oauth_applications(id);
