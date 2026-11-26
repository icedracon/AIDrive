#!/usr/bin/env bash
# Held-out set 2, lexical run: each set through the real API harness
# (tests::memory_eval_dev_set) on its own fresh database in a private cluster.
# Writes eval/heldout2/lexical-<set>.json (ranked sources, pools, latency) and
# eval/heldout2/lexical-<set>.log (the EVAL lines). Run in WSL: bash eval/heldout2/lexical.sh
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
if [[ -z "${AIDRIVE_TEST_CLUSTER:-}" ]]; then
    exec bash "$root/scripts/with-test-postgres.sh" bash "$root/eval/heldout2/lexical.sh" "$@"
fi
[[ "$PGHOST" == "$AIDRIVE_TEST_CLUSTER" && -f "$PGHOST/.aidrive-private" ]]
export AIDRIVE_MIGRATOR="${AIDRIVE_MIGRATOR:-$root/target/debug/aidrive-db}"
export AIDRIVE_APP_PASSWORD="$(od -An -N18 -tx1 /dev/urandom | tr -d ' \n')"
socket="host=$PGHOST&port=$PGPORT&sslmode=disable"
cd "$root"
cargo build --locked -q -p aidrive-db
for name in ridgeway kav volna; do
    db="aidrive_h2_$name"
    bash "$root/scripts/db-setup.sh" "$db" >/dev/null
    status=0
    out="$(AIDRIVE_EVAL_SET="$root/eval/heldout2/$name.json" AIDRIVE_EVAL_DUMP="$root/eval/heldout2/lexical-$name.json" \
        AIDRIVE_LIVE_TEST_URL="postgres://aidrive_app:$AIDRIVE_APP_PASSWORD@localhost/$db?$socket" \
        AIDRIVE_FIXTURE_ADMIN_URL="postgres://postgres:$PGPASSWORD@localhost/$db?$socket" \
        cargo test --locked -q -p aidrive-api -- --ignored --exact tests::memory_eval_dev_set --nocapture 2>&1)" || status=$?
    grep -E '^(PASS|EVAL)' <<<"$out" | tee "$root/eval/heldout2/lexical-$name.log"
    if (( status != 0 )); then
        echo "FAIL $name (exit $status)" | tee -a "$root/eval/heldout2/lexical-$name.log" >&2
        tail -n 20 <<<"$out" >&2
    fi
done
