ALTER TABLE accounts
    ALTER COLUMN discoverable DROP NOT NULL,
    ALTER COLUMN discoverable DROP DEFAULT;
