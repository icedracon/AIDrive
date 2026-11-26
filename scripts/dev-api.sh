#!/usr/bin/env bash
# Local development API against aidrive_dev (development credentials only).
# Create the database first: AIDRIVE_APP_PASSWORD=aidrive bash scripts/db-setup.sh aidrive_dev
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
export DATABASE_URL="${DATABASE_URL:-postgres://aidrive_app:aidrive@127.0.0.1:5432/aidrive_dev}"
export AIDRIVE_API_ADDR="${AIDRIVE_API_ADDR:-127.0.0.1:8080}"
exec cargo run --locked -q -p aidrive-api
