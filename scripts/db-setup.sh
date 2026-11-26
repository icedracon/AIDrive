#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
db="${1:?usage: db-setup.sh database [--adopt-initial]}"
if [[ ! "$db" =~ ^aidrive(_[a-z0-9_]+)?$ ]] || (( $# > 2 )) || [[ "${2:-}" != '' && "${2:-}" != --adopt-initial ]]; then
    echo 'Expected an aidrive database name and optional --adopt-initial' >&2
    exit 2
fi
: "${AIDRIVE_APP_PASSWORD:?Set AIDRIVE_APP_PASSWORD for first bootstrap; existing credentials are never rotated}"
migrator="${AIDRIVE_MIGRATOR:-$root/target/debug/aidrive-db}"
if [[ ! -x "$migrator" ]]; then
    echo 'Build aidrive-db first or set AIDRIVE_MIGRATOR to its executable' >&2
    exit 2
fi
psql -X -q -v ON_ERROR_STOP=1 -d postgres -v db_name="$db" -f "$root/db/bootstrap.sql"
PGDATABASE="$db" "$migrator" "$root" "${@:2}"
