-- The ten-user instance both benchmarks measure: every account follows every
-- other, and each has a token the load generators know
-- (`eunha-bench-token-<n>`).
INSERT INTO accounts (id, username, domain, display_name, note, created_at, updated_at)
SELECT i, 'user' || i, NULL, 'Benchmark User ' || i, '', now(), now()
FROM generate_series(1, 10) i;
INSERT INTO users (id, email, account_id, created_at, updated_at, confirmed_at, approved, encrypted_password)
SELECT i, 'user' || i || '@bench.invalid', i, now(), now(), now(), true, 'x'
FROM generate_series(1, 10) i;
INSERT INTO oauth_applications (id, name, uid, secret, redirect_uri, scopes, created_at, updated_at)
VALUES (1, 'benchmark', 'bench-uid', 'bench-secret', 'urn:ietf:wg:oauth:2.0:oob', 'read write follow push', now(), now());
INSERT INTO oauth_access_tokens (id, token, resource_owner_id, application_id, scopes, created_at)
SELECT i, 'eunha-bench-token-' || i, i, 1, 'read write follow push', now()
FROM generate_series(1, 10) i;
INSERT INTO follows (id, account_id, target_account_id, created_at, updated_at)
SELECT row_number() OVER (), a, b, now(), now()
FROM generate_series(1, 10) a CROSS JOIN generate_series(1, 10) b WHERE a <> b;
