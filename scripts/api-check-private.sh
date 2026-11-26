#!/usr/bin/env bash
# /readyz against real PostgreSQL: migrated + restricted role is ready;
# superuser connection and a schema missing migration 2 are not. Then the
# authenticated workspace flow against the same database.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${AIDRIVE_TEST_CLUSTER:?Private test cluster required}"
[[ "$PGHOST" == "$AIDRIVE_TEST_CLUSTER" && -f "$PGHOST/.aidrive-private" ]]
export AIDRIVE_MIGRATOR="${AIDRIVE_MIGRATOR:-$root/target/debug/aidrive-db}"
export AIDRIVE_APP_PASSWORD="$(od -An -N18 -tx1 /dev/urandom | tr -d ' \n')"

bash "$root/scripts/db-setup.sh" aidrive_ready >/dev/null
psql -X -q -v ON_ERROR_STOP=1 -d postgres -v db_name=aidrive_partial -f "$root/db/bootstrap.sql" >/dev/null
{ printf 'SET ROLE aidrive_owner;\n'; cat "$root/migrations/0001_initial.sql"; } \
    | psql -X -q -v ON_ERROR_STOP=1 -d aidrive_partial >/dev/null

socket="host=$PGHOST&port=$PGPORT&sslmode=disable"
check() {
    local label="$1" url="$2" expected="$3" out
    if ! out="$(AIDRIVE_READYZ_TEST_URL="$url" AIDRIVE_READYZ_EXPECT="$expected"         cargo test --locked -q -p aidrive-api -- --ignored --exact tests::readiness_against_live_database --nocapture 2>&1)"; then
        echo "FAIL $label" >&2
        tail -n 15 <<<"$out" >&2
        exit 1
    fi
    echo "PASS $label: $(grep -E '^(200|503) ' <<<"$out")"
}
check 'restricted role, migrated schema' \
    "postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" '"status":"ready"'
check 'superuser connection refused' \
    "postgres://postgres:$PGPASSWORD@localhost/aidrive_ready?$socket" '"role":"failed"'
check 'schema missing migration 2' \
    "postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_partial?$socket" '"schema":"failed"'
check 'wrong password' \
    "postgres://aidrive_app:wrong@localhost/aidrive_ready?$socket" '"database":"failed"'

psql -X -q -v ON_ERROR_STOP=1 -d aidrive_ready <<'SQL'
CREATE ROLE aidrive_probe LOGIN PASSWORD 'isolated-test-only';
GRANT CONNECT ON DATABASE aidrive_ready TO aidrive_probe;
GRANT USAGE ON SCHEMA public, app TO aidrive_probe;
SQL
check 'non-application role refused' \
    "postgres://aidrive_probe:isolated-test-only@localhost/aidrive_ready?$socket" '"role":"failed"'
psql -X -q -v ON_ERROR_STOP=1 -d aidrive_ready -c 'GRANT aidrive_owner TO aidrive_app'
check 'runtime role with owner membership refused' \
    "postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" '"role":"failed"'
psql -X -q -v ON_ERROR_STOP=1 -d aidrive_ready -c 'REVOKE aidrive_owner FROM aidrive_app'
check 'runtime role recovers after membership removal' \
    "postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" '"status":"ready"'

echo 'ALL READINESS CHECKS PASSED'
bash "$root/scripts/db-setup.sh" aidrive_local >/dev/null
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_local?$socket"     cargo test --locked -q -p aidrive-api -- --ignored --exact tests::local_trust_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL local trust flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    AIDRIVE_FIXTURE_ADMIN_URL="postgres://postgres:$PGPASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::files_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL durable file flow' >&2
    tail -n 35 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
"$AIDRIVE_PG_BIN/pg_ctl" -D "$AIDRIVE_TEST_CLUSTER/data" -m fast -w restart > /dev/null
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::files_survive_database_restart --nocapture 2>&1)"; then
    echo 'FAIL file restart recovery' >&2
    tail -n 25 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    AIDRIVE_FIXTURE_ADMIN_URL="postgres://postgres:$PGPASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::agents_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL agent grant flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::knowledge_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL knowledge flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::review_by_exception_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL review by exception flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::knowledge_validity_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL knowledge validity flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::precheck_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL precheck flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    AIDRIVE_FIXTURE_ADMIN_URL="postgres://postgres:$PGPASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::visibility_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL visibility flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::messages_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL messages flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket"     cargo test --locked -q -p aidrive-api -- --ignored --exact tests::chat_messages_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL chat messages flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::file_index_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL file index flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::roles_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL roles flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    AIDRIVE_FIXTURE_ADMIN_URL="postgres://postgres:$PGPASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::memory_eval_dev_set --nocapture 2>&1)"; then
    echo 'FAIL memory eval dev set' >&2
    tail -n 40 <<<"$out" >&2
    exit 1
fi
grep -E '^(PASS|EVAL)' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket"     cargo test --locked -q -p aidrive-api -- --ignored --exact tests::session_memory_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL session memory flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket"     cargo test --locked -q -p aidrive-api -- --ignored --exact tests::pairing_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL pairing flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket"     cargo test --locked -q -p aidrive-api -- --ignored --exact tests::activity_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL activity flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket"     cargo test --locked -q -p aidrive-api -- --ignored --exact tests::handoff_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL handoff flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket"     cargo test --locked -q -p aidrive-api -- --ignored --exact tests::agent_files_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL agent file flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket"     AIDRIVE_FIXTURE_ADMIN_URL="postgres://postgres:$PGPASSWORD@localhost/aidrive_ready?$socket"     cargo test --locked -q -p aidrive-api -- --ignored --exact tests::workspace_summary_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL workspace summary flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket"     cargo test --locked -q -p aidrive-api -- --ignored --exact tests::workspace_flow_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL workspace flow' >&2
    tail -n 25 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"

if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::full_password_and_logout_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL password/logout regression' >&2
    tail -n 25 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"

if ! out="$(AIDRIVE_CHUNKS_TEST_DB="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact sessions::tests::lossless_chunks_through_http_router --nocapture 2>&1)"; then
    echo 'FAIL lossless session chunks HTTP flow' >&2
    tail -n 25 <<<"$out"
    exit 1
fi
grep '^PASS' <<<"$out"

if ! out="$(AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/aidrive_ready?$socket" \
    cargo test --locked -q -p aidrive-api -- --ignored --exact tests::session_transcript_segments_against_live_database --nocapture 2>&1)"; then
    echo 'FAIL segmented session transcript flow' >&2
    tail -n 30 <<<"$out" >&2
    exit 1
fi
grep '^PASS' <<<"$out"
