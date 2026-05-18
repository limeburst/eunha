-- Migration 057: Add terms_of_service to instances
ALTER TABLE instances ADD COLUMN terms_of_service TEXT NOT NULL DEFAULT '';
