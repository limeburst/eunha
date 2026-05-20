-- Allow visibility = 4 (limited) for ActivityPub group-delivery statuses.
-- Mastodon uses this internally and masks it as "private" in API responses.
ALTER TABLE statuses DROP CONSTRAINT statuses_visibility_check;
ALTER TABLE statuses ADD CONSTRAINT statuses_visibility_check CHECK (visibility IN (0, 1, 2, 3, 4));
