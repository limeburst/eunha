ALTER TABLE tags
    ADD COLUMN IF NOT EXISTS trendable   boolean,
    ADD COLUMN IF NOT EXISTS usable      boolean,
    ADD COLUMN IF NOT EXISTS listable    boolean,
    ADD COLUMN IF NOT EXISTS reviewed_at timestamptz;
