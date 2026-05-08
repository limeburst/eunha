-- Mastodon uses a 'status' notification type (someone you follow posted).
-- Add it to the check constraint.
ALTER TABLE notifications
    DROP CONSTRAINT IF EXISTS notifications_notification_type_check;

ALTER TABLE notifications
    ADD CONSTRAINT notifications_notification_type_check
    CHECK (notification_type IN (
        'mention','reblog','favourite','follow','follow_request',
        'poll','update','status',
        'admin.sign_up','admin.report',
        'severed_relationships','moderation_warning'
    ));
