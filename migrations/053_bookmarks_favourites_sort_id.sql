-- Add BIGSERIAL sort columns to bookmarks and favourites so that
-- pagination cursors (max_id / min_id) can use integer ordering by
-- bookmark/favourite creation order rather than status ID.

CREATE SEQUENCE IF NOT EXISTS bookmark_sort_seq;
CREATE SEQUENCE IF NOT EXISTS favourite_sort_seq;

ALTER TABLE bookmarks   ADD COLUMN IF NOT EXISTS sort_id BIGINT DEFAULT nextval('bookmark_sort_seq');
ALTER TABLE favourites  ADD COLUMN IF NOT EXISTS sort_id BIGINT DEFAULT nextval('favourite_sort_seq');

-- Back-fill existing rows in created_at order so sort_id reflects
-- the original insertion order.
WITH ordered AS (
    SELECT id, row_number() OVER (ORDER BY created_at ASC) AS rn FROM bookmarks
)
UPDATE bookmarks b SET sort_id = o.rn FROM ordered o WHERE b.id = o.id;

WITH ordered AS (
    SELECT id, row_number() OVER (ORDER BY created_at ASC) AS rn FROM favourites
)
UPDATE favourites f SET sort_id = o.rn FROM ordered o WHERE f.id = o.id;

-- Set sequences to continue after the highest back-filled value.
SELECT setval('bookmark_sort_seq',  COALESCE((SELECT MAX(sort_id) FROM bookmarks),  0) + 1, false);
SELECT setval('favourite_sort_seq', COALESCE((SELECT MAX(sort_id) FROM favourites), 0) + 1, false);

-- Make the column non-nullable and default to the sequence going forward.
ALTER TABLE bookmarks   ALTER COLUMN sort_id SET NOT NULL;
ALTER TABLE bookmarks   ALTER COLUMN sort_id SET DEFAULT nextval('bookmark_sort_seq');
ALTER TABLE favourites  ALTER COLUMN sort_id SET NOT NULL;
ALTER TABLE favourites  ALTER COLUMN sort_id SET DEFAULT nextval('favourite_sort_seq');

CREATE INDEX IF NOT EXISTS idx_bookmarks_account_sort   ON bookmarks  (account_id, sort_id DESC);
CREATE INDEX IF NOT EXISTS idx_favourites_account_sort  ON favourites (account_id, sort_id DESC);
