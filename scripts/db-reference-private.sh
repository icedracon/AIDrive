#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${AIDRIVE_TEST_CLUSTER:?Private reference cluster required}"
[[ "$PGHOST" == "$AIDRIVE_TEST_CLUSTER" && -f "$PGHOST/.aidrive-private" ]]
export AIDRIVE_APP_PASSWORD=reference-only
psql -X -q -v ON_ERROR_STOP=1 -d postgres -v db_name=aidrive_reference -f "$root/db/bootstrap.sql" >/dev/null
{ printf 'SET ROLE aidrive_owner;\n'; cat "$root/migrations/0001_initial.sql"; } \
    | psql -X -q -v ON_ERROR_STOP=1 -d aidrive_reference >/dev/null
pg_dump --schema-only -d aidrive_reference
