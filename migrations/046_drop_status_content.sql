-- Mastodon does not store pre-rendered HTML in the database; it renders on-the-fly
-- from statuses.text at serve time. Matching that schema: drop the content column
-- and render HTML in the application layer instead.
ALTER TABLE statuses DROP COLUMN content;
