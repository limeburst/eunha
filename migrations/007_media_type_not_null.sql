-- The media_type generated column on media_attachments is never NULL
-- (CASE always falls through to ELSE 'unknown'), but PostgreSQL doesn't
-- track that automatically.  Add the constraint so sqlx infers String
-- rather than Option<String>.
ALTER TABLE media_attachments ALTER COLUMN media_type SET NOT NULL;
