#!/usr/bin/env bash
# Three-minute demo on a throwaway install: Claude records a decision, Codex
# changes the file it rests on, and the next brief warns that the decision is
# stale. Uses its own data and config folders; your real AIDrive is untouched.
# Needs aidrive-api and aidrive-mcp-server on PATH (or AIDRIVE_API_BIN /
# AIDRIVE_MCP) and python3.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
api_bin="${AIDRIVE_API_BIN:-aidrive-api}"
export AIDRIVE_MCP="${AIDRIVE_MCP:-aidrive-mcp-server}"
demo="$(mktemp -d)"
port="${AIDRIVE_DEMO_PORT:-8188}"
export AIDRIVE_DATA_DIR="$demo/data" XDG_CONFIG_HOME="$demo/config" APPDATA="$demo/config"
export AIDRIVE_EMBEDDED_PG_PORT="${AIDRIVE_DEMO_PG_PORT:-5588}" AIDRIVE_API_URL="http://127.0.0.1:$port"
call() { python3 "$here/mcp-call.py" "$@"; }
say() { printf '\n\033[1m%s\033[0m\n' "$*"; }

AIDRIVE_LOCAL_TRUST=1 AIDRIVE_API_ADDR="127.0.0.1:$port" "$api_bin" --embedded-db >"$demo/api.log" 2>&1 &
api=$!
trap 'kill -INT $api 2>/dev/null; wait $api 2>/dev/null; rm -rf "$demo"' EXIT
until curl -fs "127.0.0.1:$port/readyz" >/dev/null 2>&1; do sleep 1; done

mkdir -p "$demo/shop" && cd "$demo/shop"
say '$ aidrive-mcp-server connect --client claude   # and codex'
"$AIDRIVE_MCP" connect --client claude --no-register | sed -n 1p
"$AIDRIVE_MCP" connect --client codex --no-register | sed -n 1p

say 'Claude writes plan.md and records a decision citing that exact version'
file=$(call claude create_file '{"name":"plan.md","content":"# Plan\nPayments: Stripe Checkout.\n"}')
fid=$(python3 -c 'import json,sys;print(json.load(sys.stdin)["file_id"])' <<<"$file")
v1=$(python3 -c 'import json,sys;print(json.load(sys.stdin)["version_id"])' <<<"$file")
call claude propose_memory "{\"kind\":\"decision\",\"topic\":\"payments\",\"statement\":\"Use Stripe Checkout for payments\",\"evidence\":[{\"file_id\":\"$fid\",\"version_id\":\"$v1\",\"quote\":\"Payments: Stripe Checkout.\"}]}" | sed -n 1p

say 'Codex changes plan.md (it read v1, so the write is accepted as v2)'
call codex update_file "{\"file_id\":\"$fid\",\"base_version_id\":\"$v1\",\"content\":\"# Plan\nPayments: PayLink.\n\"}" | grep -o '"revision":[0-9]*' | sed -n 1p

say 'A late write from the old version is refused instead of overwriting v2'
call claude update_file "{\"file_id\":\"$fid\",\"base_version_id\":\"$v1\",\"content\":\"# Plan\nPayments: cash.\n\"}" | sed -n 1p

say 'Next session, Claude starts from the brief:'
call claude get_project_brief | sed -n '1,8p'
