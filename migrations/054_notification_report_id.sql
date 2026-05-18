-- Migration 054: Add report_id to notifications for admin.report type
ALTER TABLE notifications ADD COLUMN report_id BIGINT REFERENCES reports(id) ON DELETE CASCADE;
