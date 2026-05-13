-- Add per-follow settings mirroring Mastodon's show_reblogs / notify / languages.
ALTER TABLE follows
    ADD COLUMN IF NOT EXISTS show_reblogs BOOLEAN NOT NULL DEFAULT true,
    ADD COLUMN IF NOT EXISTS notify       BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN IF NOT EXISTS languages    TEXT[]  NOT NULL DEFAULT '{}';
