ALTER TABLE instances
    ADD COLUMN icon_url       TEXT,
    ADD COLUMN privacy_policy TEXT NOT NULL DEFAULT '',
    ADD COLUMN rules          JSONB NOT NULL DEFAULT '[]';
