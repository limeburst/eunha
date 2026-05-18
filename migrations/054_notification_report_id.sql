-- Migration 054: Add report_id to notifications for admin.report type
ALTER TABLE notifications ADD COLUMN IF NOT EXISTS report_id BIGINT REFERENCES reports(id) ON DELETE CASCADE;
