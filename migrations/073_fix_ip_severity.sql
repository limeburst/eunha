-- Fix ip_blocks.severity integers to match Mastodon's canonical enum:
--   noop=0 sign_up_requires_approval=1 sign_up_block=2 block=3
--
-- Migration 072 mapped without noop: sign_up_requires_approval=0 sign_up_block=1 block=2.
-- Shift each existing value up by 1 to make room for noop=0.

UPDATE ip_blocks SET severity = severity + 1 WHERE severity IN (0, 1, 2);

ALTER TABLE ip_blocks ALTER COLUMN severity SET DEFAULT 0;
