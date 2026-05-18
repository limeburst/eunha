-- Migration 058: Add attribution_domains to accounts
ALTER TABLE accounts ADD COLUMN attribution_domains TEXT[] NOT NULL DEFAULT '{}';
