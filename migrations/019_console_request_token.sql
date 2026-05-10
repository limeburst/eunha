ALTER TABLE console_users ADD COLUMN request_token UUID NOT NULL DEFAULT gen_random_uuid();
