-- Migration 067 incorrectly dropped NOT NULL from columns that eunha's
-- code relies on being non-null. Restore them.
-- Mastodon allows NULL for these to accommodate imported/migrated rows;
-- eunha always populates them, so we keep the NOT NULL constraints.

ALTER TABLE accounts
    ALTER COLUMN url                 SET NOT NULL,
    ALTER COLUMN fields              SET NOT NULL,
    ALTER COLUMN also_known_as       SET NOT NULL,
    ALTER COLUMN attribution_domains SET NOT NULL,
    ALTER COLUMN hide_collections    SET NOT NULL;
