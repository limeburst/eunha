#!/usr/bin/env bash
# Migrate a Mastodon pg_dump (custom format) into a fresh eunha database.
# Usage: scripts/migrate_from_mastodon.sh dump.custom [old-domain] [new-domain]
#
# Prerequisites:
#   - sqlx CLI on PATH (for running eunha schema migrations)
#   - pg_restore and psql on PATH (or set PGBIN=/path/to/pg/bin/)
#   - DATABASE_URL set (or defaults to postgres:///eunha)

set -euo pipefail

DUMP="${1:?Usage: $0 dump.custom old-domain new-domain}"
OLD="${2:?old-domain required (e.g. seoul.earth)}"
NEW="${3:?new-domain required (e.g. eunha.social)}"
DB="${DATABASE_URL:-postgres:///eunha}"
PGBIN="${PGBIN:-}"
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Extract database name from the connection string for dropdb/createdb
DBNAME="${DB##*/}"

echo "==> Recreating database '$DBNAME' ..."
"${PGBIN}dropdb" --if-exists "$DBNAME"
"${PGBIN}createdb" "$DBNAME"

echo "==> Running eunha schema migrations..."
sqlx migrate run --database-url "$DB"

echo "==> Restoring Mastodon data into $DB ..."
TOC="$(mktemp)"
"${PGBIN}pg_restore" -l "$DUMP" \
    | grep -v "TABLE DATA public ar_internal_metadata\|TABLE DATA public schema_migrations\|TABLE DATA public pghero_space_stats" \
    > "$TOC"
"${PGBIN}pg_restore" \
    --data-only \
    --no-owner \
    --no-privileges \
    --disable-triggers \
    --use-list="$TOC" \
    -d "$DB" "$DUMP" || true
rm -f "$TOC"

echo "==> Applying fixups (${OLD} -> ${NEW}) ..."
"${PGBIN}psql" "$DB" \
    -v "old_domain=${OLD}" \
    -v "new_domain=${NEW}" \
    -f "$DIR/migrate_from_mastodon.sql"

echo "==> Done."
