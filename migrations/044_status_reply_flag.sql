ALTER TABLE statuses ADD COLUMN reply BOOLEAN NOT NULL DEFAULT false;

-- Backfill: statuses with a living parent are unambiguously replies.
UPDATE statuses SET reply = true WHERE in_reply_to_id IS NOT NULL;

-- Backfill in_reply_to_account_id for migrated statuses where the parent
-- still exists in the database.
UPDATE statuses s
SET in_reply_to_account_id = p.account_id
FROM statuses p
WHERE s.in_reply_to_id = p.id
  AND s.in_reply_to_account_id IS NULL;
