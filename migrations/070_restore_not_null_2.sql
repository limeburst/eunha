-- Migration 067 incorrectly dropped NOT NULL from columns that eunha's
-- code relies on being non-null. Restore them.

ALTER TABLE announcements
    ALTER COLUMN published_at SET NOT NULL;

ALTER TABLE media_attachments
    ALTER COLUMN account_id SET NOT NULL;
