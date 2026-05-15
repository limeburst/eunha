-- Track which OAuth application created each status
ALTER TABLE statuses
    ADD COLUMN IF NOT EXISTS application_id UUID REFERENCES oauth_applications(id) ON DELETE SET NULL;
