#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if (( $# )) || [[ -n "${AIDRIVE_CHECK_DB:-}" || -n "${PGHOST_APP:-}" || -n "${PGPORT_APP:-}" ]]; then
    echo 'External test targets are not supported; db-check always creates a private PostgreSQL instance' >&2
    exit 2
fi
exec bash "$root/scripts/with-test-postgres.sh" bash "$root/scripts/db-check-private.sh"
