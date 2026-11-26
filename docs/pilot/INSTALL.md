# AIDrive pilot install

For a developer who already uses Claude Code and Codex on one project. Everything runs on your computer: PostgreSQL, the API, the web app and a small MCP server each assistant starts. Nothing is sent anywhere else. Budget 30-40 minutes, mostly compile time.

Status 2026-11-22: all five steps run on the Kali VirtualBox VM (Kali rolling, PostgreSQL 18.4, Rust 1.88, Node 24 from apt) from a fresh checkout over SSH with a real PTY. Not covered: the Claude Code and Codex apps themselves (not installed on the VM; `connect --no-register` and a JSON-RPC driver stood in for them), clicking through the web UI (the web server, proxy, one-time sign-in link and the decision endpoint were exercised with curl), and a cold cargo cache. Steps marked ⚠ are where a newcomer trips; ✔ = hit on Kali.

## Fast path: embedded PostgreSQL (2026-11-23)

No system PostgreSQL, no WSL. Rust 1.92+, from the checkout:

```bash
cargo install --locked --path apps/api           # bundles PostgreSQL 17.12 (downloaded once at build time)
cargo install --locked --path apps/mcp-server
AIDRIVE_LOCAL_TRUST=1 aidrive-api --embedded-db  # keep running; first start initialises ~/.local/share/AIDrive/postgres
```

Then continue at [step 3](#3-web-app-keep-running) (web) and [step 5](#5-connect-each-assistant-once-per-project-folder) (connect). Verified natively on Windows 11 (fresh data folder: `/readyz` ready, `connect` created the space without approval, data and the C.UTF-8 database survived restarts). Notes:

- The bundled server listens on `127.0.0.1:5544` (`AIDRIVE_EMBEDDED_PG_PORT` to change). Passwords are generated on first start and kept in the data folder (`superuser.pw`, `app.pw`); `AIDRIVE_DATA_DIR` moves everything.
- The database uses PostgreSQL's builtin `C.UTF-8` provider, so Armenian case folding works without host locales.
- Stop the API with Ctrl+C to stop PostgreSQL too. If the API is killed, PostgreSQL keeps running and the next start reuses it.
- `psql` for the metrics script is inside the data folder: `.../postgres/server/17.12.0/bin/psql -h 127.0.0.1 -p 5544 -U postgres -d aidrive` (password in `superuser.pw`).

## 0. You need (system PostgreSQL path)

| Tool | Version | Check |
|---|---|---|
| Rust | 1.92+ | `cargo --version` |
| Node.js + npm | 20.9+ | `node -v` |
| PostgreSQL server + client | 17 or 18 (18.4 verified on Kali) | `psql --version` |
| Claude Code and/or Codex CLI on `PATH` | current | `claude --version`, `codex --version` |
| git, bash | any | |

⚠ **Windows:** PostgreSQL and the API run in **WSL Ubuntu**; the MCP server runs natively on Windows. That means Rust is needed in both WSL and Windows. macOS/Linux run everything natively.

## 1. Database (once)

Linux / WSL:

```bash
sudo apt-get install -y postgresql                           # 17 or 18
sudo service postgresql start                                 # WSL has no systemd by default
sudo -u postgres createuser --superuser "$USER"               # lets you run setup as yourself
git clone <repo-url> aidrive && cd aidrive
cargo build --locked -p aidrive-db
PGHOST=/var/run/postgresql AIDRIVE_APP_PASSWORD=aidrive bash scripts/db-setup.sh aidrive_dev
# expect: "Migrations validated and applied"
```

✔ Do **not** run `db-setup.sh` via `sudo -u postgres`: home folders are mode 700 on Kali/Debian, so the postgres user cannot reach the checkout and the script reports the misleading "Build aidrive-db first". Make yourself a PostgreSQL superuser (above) instead.
✔ Rolling distros (Kali, Arch, Debian testing): after a glibc upgrade, bootstrap fails with `template database "template1" has a collation version mismatch`. Fix once: `psql -d postgres -c 'ALTER DATABASE template1 REFRESH COLLATION VERSION' -c 'ALTER DATABASE postgres REFRESH COLLATION VERSION'`.
⚠ The cluster must fold non-ASCII case. A cluster created with `--locale=C` fails readiness with `schema: failed` (Armenian case check). Default Ubuntu/Debian/Kali clusters (`C.UTF-8` or `en_US.UTF-8`) are fine. Check: `psql -d aidrive_dev -tc "select lower('Բ') = 'բ'"` → `t`.
⚠ `AIDRIVE_APP_PASSWORD=aidrive` is a development password; the API's default `DATABASE_URL` assumes it.

## 2. API (keep running)

```bash
AIDRIVE_LOCAL_TRUST=1 AIDRIVE_ALLOW_SIGNUP=1 bash scripts/dev-api.sh
curl -s 127.0.0.1:8080/readyz        # wait for "ready"; the first build takes minutes
```

⚠ Without `AIDRIVE_LOCAL_TRUST=1`, `connect` falls back to browser pairing and you must sign up and click Allow. With it, the API trusts this computer's user as the owner; it only works while the API listens on `127.0.0.1`.
⚠ `.env` / `.env.example` are **not read** by any script; pass variables on the command line as above.
⚠ WSL: if Windows builds and WSL builds share `target/`, use `CARGO_TARGET_DIR=target/wsl` for the WSL one.

## 3. Web app (keep running)

```bash
cd apps/web && npm ci && NEXT_TELEMETRY_DISABLED=1 npm run dev     # http://localhost:3000
```

✔ Without `NEXT_TELEMETRY_DISABLED=1`, Next.js prints that it "collects completely anonymous telemetry" and sends usage data to Vercel — the only outbound traffic in the install, and it contradicts "nothing is sent anywhere". (Or run `npx next telemetry disable` once.)

The web app is where you, the owner, approve or reject what assistants propose.

## 4. MCP server (once per computer)

Windows (PowerShell, from the checkout):

```powershell
cargo build --release --locked -p aidrive-mcp-server
New-Item -ItemType Directory -Force "$env:LOCALAPPDATA\AIDrive\bin" | Out-Null
Copy-Item target\release\aidrive-mcp-server.exe "$env:LOCALAPPDATA\AIDrive\bin\"
```

Linux/macOS:

```bash
cargo build --release --locked -p aidrive-mcp-server
install -D target/release/aidrive-mcp-server ~/.local/bin/aidrive-mcp-server
```

⚠ Install a *copy*. Rebuilding in place while an assistant is running the binary fails on Windows (file in use); rename the old exe first.

## 5. Connect each assistant (once per project folder)

Open your project folder in Claude Code and say:

> Run `aidrive-mcp-server connect --client claude` (on Windows the full path `%LOCALAPPDATA%\AIDrive\bin\aidrive-mcp-server.exe`).

Then the same in Codex with `--client codex`, and in OpenCode with `--client opencode` (it writes `mcp.servers.aidrive` into `~/.config/opencode/opencode.jsonc`, keeping comments and providers and backing the old file up beside it; restart OpenCode). OpenCode sessions are not captured automatically: the assistant writes a session note and a handoff before it stops.

✔ If the client's CLI is not on `PATH`, `connect` still saves the profile but prints "Add it manually: aidrive-mcp-server serve --profile claude", which is not a command you can run; the actual fix is `claude mcp add --scope user aidrive -- aidrive-mcp-server serve --profile claude` (or `codex mcp add aidrive -- ...`). Expected output: `Connected on this computer without approval (created the space ...)`. The space is named after the folder; the second assistant joins the same space. `connect` registers itself with `claude mcp add --scope user` / `codex mcp add`, so **start a new session** in each app to see the `aidrive` tools.

Check from a new session: ask the assistant to call `get_workspace`. It should show your space, `files:write`, and `profile_bound_folder` = your project.

⚠ The assistant needs permission to run a shell command; in Codex, the default sandbox may block the localhost call. Allow it once.
⚠ Unverified: whether the Claude Code **desktop** app reads `--scope user` registrations, and whether clients start the MCP server inside the project folder. If `get_workspace` shows a different `process_cwd` and the wrong space, run `aidrive-mcp-server bind --client claude --profile <name> --project <folder>`.
Assistant keys live 30 days (migration 0018; they were 7 days before 2026-11-22). After that the tools say the key expired and tell the assistant to run `connect` again; the space and its history are kept.

## 5b. Keep every session in AIDrive automatically (once per computer, Claude Code)

```powershell
& "$env:LOCALAPPDATA\AIDrive\bin\aidrive-mcp-server.exe" install-hooks
```

Each new Claude Code session in your project folder then starts with the AIDrive brief, and after every turn the session is saved (redacted on your computer, private to you and your assistants), with a session record and, at the end, a handoff draft. If a session stops at a usage limit, the next one sees where it left off. Capture is per project: `connect --capture` (or `capture-consent --client claude on` in the folder) switches it on, `capture-consent ... off` switches it off; folders bound before 2026-11-23 start switched on. Undo the hooks with `install-hooks --remove`. Codex: `install-hooks --client codex` writes SessionStart and Stop hooks to `~/.codex/hooks.json` (Codex asks you to trust them once); this is not yet observed in a real Codex session, so step 6 remains the fallback.

## 6. Optional: import earlier sessions

```powershell
aidrive-mcp-server capture --client codex --project <folder> --dry-run   # counts only
aidrive-mcp-server capture --client codex --project <folder>
```

Captured sessions are private to you until you share them in the web app. Secrets are redacted by pattern; dry-run first.

## Uninstall

`claude mcp remove aidrive`, `codex mcp remove aidrive`, delete the `aidrive` entry from `~/.config/opencode/opencode.jsonc`, delete the AIDrive config folder (`%APPDATA%\AIDrive` or `~/.config/AIDrive`), and `dropdb aidrive_dev` as the postgres user.

## Newcomer trip list (to fix before or during the pilot)

| # | Trip | Impact | Proposed fix |
|---|---|---|---|
| 1 | Keys expire on day 7 with a "ask the owner" hint | Breaks the day-7 return measurement itself | Longer default for local-trust grants or auto-renew in `connect`; fix the hint |
| 2 | Windows needs WSL Postgres + Rust in two places | Longest step; most likely drop-off | Ship a prebuilt API binary or a single `setup` script; Compose once Docker is available |
| 3 | `db-setup.sh` needs superuser + prebuilt migrator | Confusing permissions error | Wrap in `scripts/pilot-setup.sh` that builds, checks locale and bootstraps |
| 4 | `.env.example` is ignored; local trust and signup flags easy to miss | `connect` falls back to browser pairing | Default `dev-api.sh` to local trust for the pilot |
| 5 | README says the API "does not connect to PostgreSQL yet" and that storage/memory are not implemented | Stale; contradicts the product | Update README intro |
| 6 | C-locale cluster fails readiness | Opaque `schema: failed` | Mention in readiness error text |
| 7 | New session needed after `connect`; desktop app registration unverified | "Tools not showing" | Say it in `connect` output; verify desktop |
| 8 | No prebuilt binaries | 10+ minutes compile | Release artifacts after first pilot round |
| 9 | `sudo -u postgres bash scripts/db-setup.sh` cannot read a 700 home; error says "Build aidrive-db first" (Kali, observed) | Newcomer rebuilds in a loop | Check readability separately and say so; document `createuser --superuser $USER` |
| 10 | Rolling-distro collation mismatch breaks `CREATE DATABASE` (Kali, observed) | Hard stop at step 1 | Detect in `db-setup.sh` and print the REFRESH command |
| 11 | README requires PostgreSQL 17; Kali ships only 18 | Newcomer hunts for PG 17 | 18.4 passes setup + readiness; say "17+" |
| 12 | `connect` without the client CLI prints a non-command as the manual fix (observed) | Tools never appear | Print the exact `claude mcp add` / `codex mcp add` line |
| 14 | Next.js dev telemetry is on by default (observed) | Breaks the "local only" promise | Set `NEXT_TELEMETRY_DISABLED=1` in the web `dev` script |
| 13 | `open` on a machine without a browser prints six `xdg-open` errors after the link (observed) | Noise, link still printed | Silence the opener's stderr |

## Security scan (owner-approved only)

Not run yet. Downloads; ask the owner first. Pinned versions only, never `@latest`.

```powershell
npx -y @modelcontextprotocol/inspector@2.14.0 --cli "$env:LOCALAPPDATA\AIDrive\bin\aidrive-mcp-server.exe" serve --profile claude --method tools/list
```

```powershell
uvx mcp-scan@<version pinned from PyPI> inspect
```

`mcp-scan scan` (not `inspect`) uploads tool names and descriptions to the vendor's API; use it only with separate approval.
