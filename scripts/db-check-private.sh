#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${AIDRIVE_TEST_CLUSTER:?Private test cluster required}"
[[ "$PGHOST" == "$AIDRIVE_TEST_CLUSTER" && -f "$PGHOST/.aidrive-private" ]]
[[ "$(psql -X -At -c 'SHOW data_directory')" == "$AIDRIVE_TEST_CLUSTER/data" ]]
export AIDRIVE_MIGRATOR="${AIDRIVE_MIGRATOR:-$root/target/debug/aidrive-db}"
[[ -x "$AIDRIVE_MIGRATOR" ]]
export AIDRIVE_APP_PASSWORD="$(od -An -N18 -tx1 /dev/urandom | tr -d ' \n')"
original_password="$AIDRIVE_APP_PASSWORD"
scratch="$AIDRIVE_TEST_CLUSTER/cases"
mkdir "$scratch"
expect_failure() {
    local label="$1" expected="$2"
    shift 2
    if "$@" >"$scratch/failure.log" 2>&1; then
        echo "FAIL $label: command unexpectedly succeeded" >&2
        exit 1
    fi
    if ! grep -Fq "$expected" "$scratch/failure.log"; then
        echo "FAIL $label: did not observe expected rejection" >&2
        cat "$scratch/failure.log" >&2
        exit 1
    fi
    echo "PASS $label"
}
bootstrap() {
    psql -X -q -v ON_ERROR_STOP=1 -d postgres -v db_name="$1" -f "$root/db/bootstrap.sql"
}
app_sql() {
    PGPASSWORD="$original_password" psql -X -q -v ON_ERROR_STOP=1 -U aidrive_app -d "$1" "${@:2}"
}

bootstrap aidrive_legacy
{ printf 'SET ROLE aidrive_owner;\n'; cat "$root/migrations/0001_initial.sql"; } \
    | psql -X -q -v ON_ERROR_STOP=1 -d aidrive_legacy
app_sql aidrive_legacy -f "$root/db/tests/isolation.sql"
workspace="$(psql -X -At -d aidrive_legacy -c "SELECT id FROM workspaces WHERE name = 'A project'")"
expect_failure 'attribution regression fails before fix' 'FAIL creator attribution was rewritten' \
    app_sql aidrive_legacy -v workspace_id="$workspace" -f "$root/db/tests/provenance.sql"
expect_failure 'untracked legacy schema refused' 'existing untracked schema' \
    env PGDATABASE=aidrive_legacy "$AIDRIVE_MIGRATOR" "$root"

bootstrap aidrive_drift
{ printf 'SET ROLE aidrive_owner;\n'; cat "$root/migrations/0001_initial.sql"; printf '\nGRANT UPDATE ON workspaces TO PUBLIC;\n'; } \
    | psql -X -q -v ON_ERROR_STOP=1 -d aidrive_drift
expect_failure 'legacy policy/grant drift blocks adoption' 'legacy schema differs' \
    env PGDATABASE=aidrive_drift "$AIDRIVE_MIGRATOR" "$root" --adopt-initial
[[ "$(psql -X -At -d aidrive_drift -c "SELECT to_regclass('public._sqlx_migrations') IS NULL")" == t ]]

before="$(psql -X -At -d aidrive_legacy -c "SELECT md5(string_agg((to_jsonb(f) - 'current_version_id')::text, ',' ORDER BY id)) FROM files f")"
PGDATABASE=aidrive_legacy "$AIDRIVE_MIGRATOR" "$root" --adopt-initial
after="$(psql -X -At -d aidrive_legacy -c "SELECT md5(string_agg((to_jsonb(f) - 'current_version_id')::text, ',' ORDER BY id)) FROM files f")"
[[ "$before" == "$after" ]]
[[ "$(psql -X -At -d aidrive_legacy -c 'SELECT count(*) FROM files WHERE current_version_id IS NOT NULL')" == 0 ]]
app_sql aidrive_legacy -v workspace_id="$workspace" -f "$root/db/tests/provenance.sql"
echo 'PASS verified baseline adoption and upgrade preserve existing rows'

bash "$root/scripts/db-setup.sh" aidrive_fresh
app_sql aidrive_fresh -f "$root/db/tests/isolation.sql"
workspace="$(psql -X -At -d aidrive_fresh -c "SELECT id FROM workspaces WHERE name = 'A project'")"
app_sql aidrive_fresh -v workspace_id="$workspace" -f "$root/db/tests/provenance.sql"
ledger_before="$(psql -X -At -d aidrive_fresh -c "SELECT md5(string_agg(row_to_json(m)::text, ',' ORDER BY version)) FROM _sqlx_migrations m")"
AIDRIVE_APP_PASSWORD=different-should-not-rotate bash "$root/scripts/db-setup.sh" aidrive_fresh
app_sql aidrive_fresh -c 'SELECT current_user' >/dev/null
expect_failure 'bootstrap does not rotate the application login' 'password authentication failed' \
    env PGPASSWORD=different-should-not-rotate psql -X -U aidrive_app -d aidrive_fresh -c 'SELECT 1'
ledger_after="$(psql -X -At -d aidrive_fresh -c "SELECT md5(string_agg(row_to_json(m)::text, ',' ORDER BY version)) FROM _sqlx_migrations m")"
[[ "$ledger_before" == "$ledger_after" ]]
echo 'PASS repeat setup is a no-op for applied migrations and existing credentials'
app_sql aidrive_fresh -f "$root/db/tests/auth.sql"
psql -X -q -v ON_ERROR_STOP=1 -d aidrive_fresh -c "UPDATE auth_sessions SET last_seen_at = statement_timestamp() - interval '25 hours' WHERE token_hash = decode(repeat('bb', 32), 'hex')"     -c "UPDATE auth_sessions SET expires_at = statement_timestamp() - interval '1 second' WHERE token_hash = decode(repeat('cc', 32), 'hex')"
[[ "$(app_sql aidrive_fresh -At -c "SELECT app.resolve_session(decode(repeat('bb', 32), 'hex')) IS NULL AND app.resolve_session(decode(repeat('cc', 32), 'hex')) IS NULL")" == t ]]
echo 'PASS idle (24h) and absolute (7d) session expiry enforced'
app_sql aidrive_fresh -f "$root/db/tests/workspaces.sql"
app_sql aidrive_fresh -f "$root/db/tests/agent_files.sql"
app_sql aidrive_fresh -f "$root/db/tests/handoffs.sql"
app_sql aidrive_fresh -f "$root/db/tests/activity.sql"
app_sql aidrive_fresh -f "$root/db/tests/pairing.sql"
app_sql aidrive_fresh -f "$root/db/tests/sessions.sql"
psql -X -q -v ON_ERROR_STOP=1 -d aidrive_fresh -f "$root/db/tests/session_chunks.sql"
app_sql aidrive_fresh -f "$root/db/tests/knowledge.sql"
app_sql aidrive_fresh -f "$root/db/tests/knowledge_validity.sql"
app_sql aidrive_fresh -f "$root/db/tests/visibility.sql"
app_sql aidrive_fresh -f "$root/db/tests/messages.sql"
app_sql aidrive_fresh -f "$root/db/tests/chat_messages.sql"
psql -X -q -v ON_ERROR_STOP=1 -d aidrive_fresh -f "$root/db/tests/security_review.sql"
app_sql aidrive_fresh -f "$root/db/tests/key_lifetime.sql"
app_sql aidrive_fresh -f "$root/db/tests/file_index.sql"
app_sql aidrive_fresh -f "$root/db/tests/file_index_sizes.sql"
app_sql aidrive_fresh -f "$root/db/tests/file_names.sql"
app_sql aidrive_fresh -f "$root/db/tests/roles.sql"
psql -X -q -v ON_ERROR_STOP=1 -d aidrive_fresh     -c "UPDATE memberships m SET role = 'owner' FROM users u, workspaces w WHERE m.user_id = u.id AND m.workspace_id = w.id AND u.email = 'role-editor@example.test' AND w.name = 'Roles'"     -c "UPDATE memberships m SET role = 'editor' FROM users u, workspaces w WHERE m.user_id = u.id AND m.workspace_id = w.id AND u.email = 'role-owner@example.test' AND w.name = 'Roles'"
roles_owner="$(psql -X -At -d aidrive_fresh -c "SELECT id FROM users WHERE email = 'role-owner@example.test'")"
roles_ws="$(psql -X -At -d aidrive_fresh -c "SELECT id FROM workspaces WHERE name = 'Roles'")"
app_sql aidrive_fresh -v owner_id="$roles_owner" -v ws="$roles_ws" -f "$root/db/tests/roles_demoted.sql"
app_sql aidrive_fresh -f "$root/db/tests/knowledge_review.sql"
mkdir -p "$scratch/through-0012/migrations"
cp "$root"/migrations/00{01..12}_*.sql "$scratch/through-0012/migrations/"
bootstrap aidrive_backfill
PGDATABASE=aidrive_backfill "$AIDRIVE_MIGRATOR" "$scratch/through-0012"
app_sql aidrive_backfill -f "$root/db/tests/knowledge_backfill_seed.sql"
PGDATABASE=aidrive_backfill "$AIDRIVE_MIGRATOR" "$root"
psql -X -q -v ON_ERROR_STOP=1 -d aidrive_backfill -f "$root/db/tests/knowledge_backfill.sql"
mkdir -p "$scratch/through-0017/migrations"
cp "$root"/migrations/00{01..17}_*.sql "$scratch/through-0017/migrations/"
bootstrap aidrive_keys
PGDATABASE=aidrive_keys "$AIDRIVE_MIGRATOR" "$scratch/through-0017"
app_sql aidrive_keys -f "$root/db/tests/key_lifetime_backfill_seed.sql"
psql -X -q -v ON_ERROR_STOP=1 -d aidrive_keys -c "UPDATE agent_grants SET created_at = clock_timestamp() - interval '2 days', expires_at = clock_timestamp() + interval '5 days' WHERE name = 'B-valid'"     -c "UPDATE agent_grants SET created_at = clock_timestamp() - interval '10 days', expires_at = clock_timestamp() - interval '3 days' WHERE name = 'B-expired'"     -c "UPDATE agent_grants SET revoked_at = clock_timestamp() WHERE name = 'B-revoked'"
PGDATABASE=aidrive_keys "$AIDRIVE_MIGRATOR" "$root"
psql -X -q -v ON_ERROR_STOP=1 -d aidrive_keys -f "$root/db/tests/key_lifetime_backfill.sql"
race_ws="$(psql -X -At -d aidrive_fresh -c "SELECT w.id FROM workspaces w WHERE w.name = 'Owner one'")"
psql -X -q -v ON_ERROR_STOP=1 -d aidrive_fresh -c "UPDATE memberships SET role = 'owner' WHERE workspace_id = '$race_ws'"
mapfile -t race_users < <(psql -X -At -d aidrive_fresh -c "SELECT user_id FROM memberships WHERE workspace_id = '$race_ws' ORDER BY user_id")
[[ "${#race_users[@]}" == 2 ]]
psql -X -q -d aidrive_fresh -c "BEGIN; UPDATE memberships SET role = 'viewer' WHERE workspace_id = '$race_ws' AND user_id = '${race_users[0]}'; SELECT pg_sleep(1); COMMIT;" > "$scratch/race1.log" 2>&1 &
race1=$!
sleep 0.3
psql -X -q -d aidrive_fresh -c "UPDATE memberships SET role = 'viewer' WHERE workspace_id = '$race_ws' AND user_id = '${race_users[1]}'" > "$scratch/race2.log" 2>&1 || true
wait "$race1" || true
[[ "$(psql -X -At -d aidrive_fresh -c "SELECT count(*) FROM memberships WHERE workspace_id = '$race_ws' AND role = 'owner'")" == 1 ]]
grep -q 'at least one owner' "$scratch/race2.log"
echo 'PASS concurrent demotions of two owners leave exactly one owner'
expect_failure 'runtime role cannot migrate' 'migration connection must' \
    env PGUSER=aidrive_app PGPASSWORD="$original_password" PGDATABASE=aidrive_fresh "$AIDRIVE_MIGRATOR" "$root"

mkdir -p "$scratch/changed/migrations" "$scratch/failed/migrations" "$scratch/concurrent/migrations"
cp "$root"/migrations/*.sql "$scratch/changed/migrations/"
printf '\n-- checksum regression\n' >> "$scratch/changed/migrations/0001_initial.sql"
expect_failure 'modified applied migration refused' 'VersionMismatch(1)' \
    env PGDATABASE=aidrive_fresh "$AIDRIVE_MIGRATOR" "$scratch/changed"
cp "$root"/migrations/*.sql "$scratch/failed/migrations/"
printf 'CREATE TABLE public.should_rollback(id int); SELECT 1/0;\n' > "$scratch/failed/migrations/0900_failure.sql"
expect_failure 'failed migration reports an error' 'division by zero' \
    env PGDATABASE=aidrive_fresh "$AIDRIVE_MIGRATOR" "$scratch/failed"
[[ "$(psql -X -At -d aidrive_fresh -c "SELECT to_regclass('public.should_rollback') IS NULL AND NOT EXISTS (SELECT 1 FROM _sqlx_migrations WHERE version = 900)")" == t ]]
PGDATABASE=aidrive_fresh "$AIDRIVE_MIGRATOR" "$root"
echo 'PASS failed migration rolls back DDL/ledger and releases its lock'

cp "$root"/migrations/*.sql "$scratch/concurrent/migrations/"
printf 'SELECT pg_sleep(2); CREATE TABLE public.applied_once(id int);\n' > "$scratch/concurrent/migrations/0901_once.sql"
PGDATABASE=aidrive_fresh "$AIDRIVE_MIGRATOR" "$scratch/concurrent" > "$scratch/first.log" 2>&1 &
first_pid=$!
PGDATABASE=aidrive_fresh "$AIDRIVE_MIGRATOR" "$scratch/concurrent" > "$scratch/second.log" 2>&1 &
second_pid=$!
wait "$first_pid"
wait "$second_pid"
[[ "$(psql -X -At -d aidrive_fresh -c 'SELECT count(*) FROM _sqlx_migrations WHERE version = 901 AND success')" == 1 ]]
echo 'PASS concurrent migration runners apply a version once'

expect_failure 'external database override refused' 'External test targets are not supported' \
    env AIDRIVE_CHECK_DB=aidrive bash "$root/scripts/db-check.sh"
echo 'ALL FOUNDATION CHECKS PASSED (private cluster; no external database target)'
