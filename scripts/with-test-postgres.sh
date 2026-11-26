#!/usr/bin/env bash
set -euo pipefail
umask 077
if (( EUID == 0 )); then
    echo 'Run as a non-root OS user; PostgreSQL initdb refuses root' >&2
    exit 2
fi
pg_bin="${AIDRIVE_PG_BIN:-$(pg_config --bindir)}"
if [[ ! -x "$pg_bin/initdb" || ! -x "$pg_bin/pg_ctl" || ! -x "$pg_bin/pg_dump" ]]; then
    echo 'Set AIDRIVE_PG_BIN to the PostgreSQL server/client bin directory' >&2
    exit 2
fi
export PATH="$pg_bin:$PATH"
private_dir="$(mktemp -d /tmp/aidrive-pg.XXXXXXXX)"
private_dir="$(realpath "$private_dir")"
cleanup() {
    local status=$?
    if [[ -f "$private_dir/data/postmaster.pid" ]]; then
        if ! pg_ctl -D "$private_dir/data" -m immediate -w stop >/dev/null 2>&1; then
            echo "Could not stop private PostgreSQL; preserved $private_dir" >&2
            return
        fi
    fi
    if [[ "$private_dir" == /tmp/aidrive-pg.* && -f "$private_dir/.aidrive-private" ]]; then
        if (( status != 0 )); then
            echo 'Private test failure; server log:' >&2
            tail -n 12 "$private_dir/server.log" >&2 2>/dev/null || true
        fi
        rm -rf -- "$private_dir"
    fi
}
touch "$private_dir/.aidrive-private"
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
unset PGHOST PGHOSTADDR PGPORT PGUSER PGPASSWORD PGDATABASE PGSERVICE PGSERVICEFILE PGPASSFILE PGOPTIONS PGSSLMODE
export AIDRIVE_PG_BIN="$pg_bin"
export PGHOST="$private_dir" PGPORT=5432 PGUSER=postgres PGDATABASE=postgres PGSSLMODE=disable
export PGPASSWORD="$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')"
printf '%s\n' "$PGPASSWORD" > "$private_dir/password"
initdb -D "$private_dir/data" -U postgres --encoding=UTF8 --locale=C.UTF-8 \
    --auth-local=scram-sha-256 --auth-host=reject --pwfile="$private_dir/password" >/dev/null
pg_ctl -D "$private_dir/data" -l "$private_dir/server.log" \
    -o "-c listen_addresses='' -c unix_socket_directories='$private_dir' -c unix_socket_permissions=0700" -w start >/dev/null
export AIDRIVE_TEST_CLUSTER="$private_dir"
"$@"
