# AIDrive reference

The detailed reference moved here from the README, unchanged. The README is the short overview.

## Install in detail

The release is a folder you can send as one zip; nothing is hosted yet. Extract it, open a terminal in your project folder and run its installer. It needs no internet, admin rights, Rust, Node.js, Docker, WSL or system PostgreSQL:

```powershell
C:\path\to\AIDrive\install.cmd               # Windows
```

```bash
sh /path/to/AIDrive/install.sh               # Linux
```

Once a release is hosted, the same scripts install with one line: `irm <release-url>/install.ps1 | iex` or `curl -fsSL <release-url>/install.sh | sh`.

The installer checks the archive against `SHA256SUMS`, puts `aidrive` and `aidrive-mcp-server` in `%LOCALAPPDATA%\AIDrive\bin` or `~/.local/share/AIDrive/bin` (linked from `~/.local/bin`), and runs `aidrive up`, which:

1. starts AIDrive in the background: the API with a bundled PostgreSQL 17 and the web app, on `127.0.0.1` only (API and web app on port 7480, PostgreSQL on 7481, or the next free ports; kept in `aidrive.json` in the data folder). The first start unpacks PostgreSQL;
2. installs the session hooks once for Claude Code and Codex if found (`aidrive-mcp-server install-hooks --client claude|codex`);
3. connects Claude Code, Codex and OpenCode to a space named after the current folder (`aidrive-mcp-server connect --client … --project <folder>`), asking once whether to capture every Claude Code / Codex session in that folder (`--capture`). Without a terminal it does not ask: set `AIDRIVE_CAPTURE=on` or `off`, or pass `--capture` / `--no-capture`. OpenCode has no hooks, so its sessions are not captured.

Then open http://localhost:7480 (it opens as the owner on this computer) and start a new session in the folder.

```text
aidrive up [--project DIR] [--capture|--no-capture] [--no-connect]   # safe to re-run; another folder: run it there
aidrive status                                                          # what runs where, connected folders
aidrive down                                                            # stop; data stays
aidrive uninstall                                                       # stop, remove hooks and MCP entries; lists folders to delete
```

Update: run the newer release's installer the same way; it stops AIDrive, replaces the programs and starts it again, and the data folder is kept (migrations run on start).

Data lives in `%LOCALAPPDATA%\AIDrive` or `~/.local/share/AIDrive` (`AIDRIVE_DATA_DIR` moves it); the log is `logs/api.log` there. Installer settings: `AIDRIVE_RELEASE_URL` (a URL, a local release folder or the archive itself), `AIDRIVE_INSTALL_DIR`, `AIDRIVE_NO_PATH=1` (Windows: leave PATH alone). Codex hooks are installed but not yet observed in a real Codex session.

Build the release folder with `bash scripts/package-release.sh` (Rust 1.92+, Node.js 20.9+); building on Windows and then Linux into the same `AIDRIVE_DIST_DIR` gives one folder for both. Testers get `READ-ME-FIRST.txt` (RU/EN) in it.

### From source (developers)

```bash
cargo install --locked --path apps/api          # aidrive-api, PostgreSQL 17 bundled inside
cargo install --locked --path apps/mcp-server   # aidrive-mcp-server
AIDRIVE_LOCAL_TRUST=1 aidrive-api --embedded-db # keep running
```

`--embedded-db` starts a private PostgreSQL 17 that is bundled into the binary at build time (the build downloads it once from [theseus-rs/postgresql-binaries](https://github.com/theseus-rs/postgresql-binaries)), keeps its data in `%LOCALAPPDATA%\AIDrive\postgres` or `~/.local/share/AIDrive/postgres`, listens only on `127.0.0.1:5544`, and applies migrations on every start. Without the flag the API uses `DATABASE_URL` as before ([pilot install guide](pilot/INSTALL.md)). Then, in each project folder, run `aidrive-mcp-server connect --client claude` (or `--client codex`, add `--capture` to capture sessions) and `aidrive-mcp-server install-hooks` once. The web app is in `apps/web`: `npm ci && NEXT_TELEMETRY_DISABLED=1 npm run dev`, then http://localhost:3000.



## Requirements (from source)

- Rust 1.92 or newer
- Node.js 20.9 or newer and npm (web app)
- PostgreSQL 17 or 18 server tools with a UTF-8 locale, only for `scripts/db-check.sh` / `api-check.sh` and the non-embedded setup. Local credentials are for development only.

## Start the API

```powershell
cargo run -p aidrive-api
```

The API listens on `127.0.0.1:8080` by default. Set `AIDRIVE_API_ADDR` to change the bind address. Set `DATABASE_URL` to an `aidrive_app` connection (never a superuser or owner): `/healthz` reports liveness only, `/readyz` returns 503 until the database, role and schema checks pass. `bash scripts/api-check.sh` verifies readiness against a disposable PostgreSQL instance. Authentication (ADR 0002): set `AIDRIVE_ALLOW_SIGNUP=1` to permit account creation and `AIDRIVE_WEB_ORIGIN` if the web app is not on `http://localhost:3000`.

## Start the local MCP server

```powershell
cargo run -p aidrive-mcp-server
```

It uses stdio. Status tools: `get_api_health`, `get_api_readiness`. Shared-workspace tools (need `AIDRIVE_AGENT_TOKEN` and `AIDRIVE_WORKSPACE_ID` from an agent key created in the web app): `get_workspace`, `list_files`, `read_file`, `file_history`, `create_file`, `update_file`, `get_latest_handoff`, `record_handoff`, `get_activity`. Handoffs stay proposed until the owner approves them in the web app, and are flagged stale when a cited file changes. Writes are conditional on the version you read and are recorded as the issuing owner plus the agent. Agent keys have a scope level: `workspace:read` < `files:read` < `files:write`. For a persistent local API, run `bash scripts/dev-api.sh` in WSL after creating `aidrive_dev` with `scripts/db-setup.sh`. Set `AIDRIVE_API_URL` if the API is listening somewhere other than `http://127.0.0.1:8080`.

## Start PostgreSQL

With PostgreSQL 17 server/client tools installed in WSL/Linux, run the isolated checks as your normal non-root user:

```bash
cargo build --locked -p aidrive-db
bash scripts/db-check.sh
```

The checks create their own private PostgreSQL instance; they do not require or recreate your development database. For actual setup, configure administrator `PGHOST`/`PGPORT`/`PGUSER`/`PGPASSWORD`, set `AIDRIVE_APP_PASSWORD`, then run `bash scripts/db-setup.sh aidrive`. Existing application credentials are preserved. Pre-ledger databases require explicit verified adoption during maintenance; see [docs/DATABASE.md](DATABASE.md).

`docker compose up -d db` bootstraps roles/database only; run the migration utility from the host afterward. Compose remains unverified. Runtime API access must use `aidrive_app`.

## Connect Claude Code, Codex or OpenCode (pairing)

Build once and install a stable copy, so rebuilding never breaks a connected app:

```powershell
cargo build --release -p aidrive-mcp-server
New-Item -ItemType Directory -Force "$env:LOCALAPPDATA\AIDrive\bin" | Out-Null
Copy-Item target\release\aidrive-mcp-server.exe "$env:LOCALAPPDATA\AIDrive\bin\"
```

Then, in the desktop app, ask the assistant to run (with a 10-minute timeout):

```powershell
& "$env:LOCALAPPDATA\AIDrive\bin\aidrive-mcp-server.exe" connect --client claude   # or --client codex
```

It prints a code and opens the AIDrive approval page; the owner picks the space and permission and clicks Allow. The command saves a per-user profile (`%APPDATA%\AIDrive\profiles\<client>.json`) and registers itself with `claude mcp add --scope user` or `codex mcp add`, pointing at `serve --profile <client>`, so no key appears in either app's config. Start a new session in the app to use the tools. If the command is interrupted, running it again resumes the same request within 9 minutes.

OpenCode (desktop app or CLI) has no non-interactive `mcp add`, so `connect --client opencode` writes the entry into OpenCode's global config `~/.config/opencode/opencode.jsonc` (or `opencode.json`; on Windows `%USERPROFILE%\.config\opencode`). Only the `aidrive` entry changes: comments, providers and other servers stay byte for byte, and the previous file is kept beside it as `opencode.jsonc.aidrive-<time>.bak`. OpenCode 2.x gets `mcp.servers.aidrive = {"type": "local", "command": [exe, "serve", "--profile", "opencode"]}`; OpenCode 1.x (which rejects `mcp.servers`) gets `mcp.aidrive` with `"enabled": true`; the newest OpenCode found on the computer decides. OpenCode gets its own key, shown as "OpenCode" in activity, and its own profiles (`opencode`, `opencode-<space>`); it never reuses a Claude or Codex profile. Restart OpenCode afterwards. Claude desktop is an MSIX app: whatever it (or a program it starts, WSL included) writes to `%APPDATA%\AIDrive` lands in `%LOCALAPPDATA%\Packages\<package>\LocalCache\Roaming\AIDrive`, which OpenCode and the Codex app cannot see. The MCP server therefore also reads profiles, bindings and capture consent from Anthropic-signed Claude package copies (its own folder wins), and `connect --client opencode` writes the exe path as OpenCode sees it. OpenCode sessions are not captured automatically yet: AIDrive's session store accepts Claude Code and Codex transcripts only, so in OpenCode the assistant writes a session note and a handoff before it stops (`get_workspace` says so).

`serve --profile <name>` with a profile name that is not a client (for example `claude-aidrive-launch`) uses exactly that profile, wherever the session runs, without the "not bound" warning.

One computer, several projects: run `connect` from each project folder (or pass `--project <folder>`). The first space becomes the client's default profile; another space is saved as `<client>-<space>` and bound to that folder in `%APPDATA%\AIDrive\profiles\bindings.json`, so connecting a second project never re-binds the first. `serve --profile claude` then uses the profile bound to the nearest enclosing folder of its working directory, otherwise the default. `aidrive-mcp-server profiles` lists profiles and bindings (never keys); `aidrive-mcp-server bind --client claude --profile <name> --project <folder>` binds a folder to an existing profile. `get_workspace` reports which profile a session uses and why.

## Capture assistant sessions (shared memory)

After pairing, import the Claude Code or Codex sessions that ran inside one project folder, so any connected assistant can search what was discussed:

```powershell
$aidrive = "$env:LOCALAPPDATA\AIDrive\bin\aidrive-mcp-server.exe"
& $aidrive capture --client codex --project "C:\path\to\project" --dry-run   # counts only, uploads nothing
& $aidrive capture --client codex --project "C:\path\to\project"
```

Only sessions whose working directory is inside `--project` are read. What leaves the machine: what the user and assistant said and short tool notes (`tool: Edit plan.md`). Model reasoning, system/developer instructions, tool output and Codex's internal reviewer threads are dropped. Text is redacted locally before upload (labelled secrets, bearer tokens, URL credentials, private keys, common token shapes); an unlabelled secret with no recognisable shape can still slip through, so dry-run first. Re-running is idempotent. The owner can delete any captured session in the web app.

Automatic capture for Claude Code: a `Stop` hook can run `"%LOCALAPPDATA%\AIDrive\bin\aidrive-mcp-server.exe" capture --client claude --project "C:\path\to\project" --hook`, which reads the hook's JSON (`transcript_path`) from stdin and still only uploads sessions inside `--project`. The hook path has unit coverage but has not yet been exercised from an installed Claude Code hook.

MCP tools for memory: `get_project_brief` (start here: rules, approved decisions, what was tried, what waits for the owner, the handoff, what changed since this key's last action, recent activity and sessions), `precheck_file` (before editing: rules, failed attempts and decisions that cite the file, handoffs built on it, sessions that mentioned it and whether it changed since; `update_file` also lists the approved knowledge an edit just marked for re-checking), `search_sessions` (budgeted, ranked snippets), `get_session_excerpt`, `search_memory` (approved knowledge with evidence, stale and disputed flags; `include_history` for proposals and superseded wording; `as_of` for what was agreed at an earlier moment), `propose_memory` (decision, fact, constraint, preference, gotcha, attempt with outcome, or keystone rule, citing an exact file version, session moment or handoff).

In strict review, proposals wait in the review inbox for an authorized decision. Under review by exception, eligible proposals and handoffs are accepted automatically, labelled as not checked by the owner, and can be kept or undone. The owner and appropriately delegated assistant roles can decide shared records within their permissions; private items remain with their owner. Accepting a handoff also records its decisions. An item whose cited file has changed is flagged for re-checking and cannot be approved until it is re-proposed. A proposal with the same kind and topic as approved knowledge is a conflict: the approved item shows as disputed to every assistant, and the owner must choose to replace it or keep both. The owner can also retire an item that no longer holds; nothing is deleted, and earlier states stay available through `as_of`.

Visibility: captured sessions are private by default (only the person who captured them and the assistants acting for that person; other members, including workspace owners, do not see them) until their creator shares one with the team in the web app. Knowledge is team by default; an assistant can propose a private item (`propose_memory` with `visibility: private`), which only its creator sees and decides.

MCP tools for coordination: `send_message` (to all assistants, the owner, or one assistant key by name; can reply in a thread and cite exact file versions) and `get_messages` (unread messages for you, marked read when returned; or a whole thread). Unread messages for an assistant also appear in `get_project_brief`. A message is a request to weigh, not approved project state; finished work still goes through `record_handoff`.

Chat push (migration 0022, `install-hooks` for each client): every chat registers itself in the space its MCP server uses. With hooks, each Claude Code session and Codex thread registers by its own session id; without hooks (OpenCode, or hooks not installed) the MCP server registers its process as the chat. `get_workspace` and the brief tell a chat its id and how others reach it (`send_message to=<key> to_chat=<id>`). Without hooks, messages for that chat arrive through `get_messages`. A message is pushed into one chat by the client's hooks, so nobody has to relay "read your messages". Every message to one assistant names its chat: `send_message to=<key> to_chat=<id>` (ids from `list_chats`), and a reply (`reply_to`) goes back to the chat the original came from. If an assistant writes to a key that has several chats active in the last 24 hours without naming one, the message is refused and the refusal lists that key's chats. A key with a single chat gets the message in that chat. The owner may still write to a key; then the first active chat of that key takes it, and only that chat. Delivered messages show the sender's chat id. Delivered text is framed as data from another assistant, never an instruction. In Claude Code, messages are added at session start, on each prompt and after each tool, and a background Stop hook (`asyncRewake`) wakes an idle chat when a message arrives. In Codex, messages are added at thread start, on each prompt and after each tool, and the Stop hook continues the turn so Codex answers. An idle Codex thread has no documented way to be woken, so it gets its messages on its next prompt. Codex runs new hooks only after you trust them in `/hooks`. Limits: a chat is woken at most once every 30 seconds, and after 6 automatic wakes in a thread with no message from the owner, AIDrive stops waking chats in that thread and tells the owner. Pending messages then wait for each chat's next turn, and a reply from the owner in the thread lets the assistants continue. `get_messages` shows which chat took each message.

File index: an assistant that creates a file outside AIDrive (a scratch script, probe, report, export, temporary data, a worktree) registers it with `register_file` (purpose and `keep_days`, default 3); AIDrive records the machine, absolute path, git worktree and branch. After the deadline, `get_project_brief` checks on that machine whether each file still exists and asks for a keep, extend or removal decision. Expiry alone does not authorize deletion; check ownership and current use before approved removal, then record `resolve_file ... removed`; a file found gone is recorded as missing. `list_registered_files` lists them. Build and package folders in bound folders are indexed automatically with their size, and when free space runs low the brief lists the biggest registered items, whether they still look in use, and what removing them would free, then asks. AIDrive never deletes files itself. These rules, together with precheck, messages and capture, are in the MCP server instructions every assistant receives on connect.

Assistant roles: when the owner issues a key (web app, Manage AI access, or a pairing approval) they choose its role. Low (`files:write`) reads, edits, proposes and hands off. Medium (`review:accept`) can also approve or reject other assistants' proposals and handoffs (`list_pending`, `decide_proposal`, `decide_handoff`), never its own. Grand can do everything the owner does with project state: accept its own work, retire knowledge, keep or undo auto-accepted items, change the review mode; it never issues keys or manages members. No assistant decides private items, a role above low only counts while its issuer still owns the space, local trust never issues more than low, and the activity timeline names the key that made each decision.

## Automatic session state (no handoff needed)

```powershell
& "$env:LOCALAPPDATA\AIDrive\bin\aidrive-mcp-server.exe" install-hooks   # --remove to undo
```

This adds Claude Code hooks to `~/.claude/settings.json` (backed up once as `settings.json.aidrive-backup`; other hooks are kept). At session start, resume and after compaction the AIDrive brief is put into the session's context; after every turn, before compaction and at session end the new part of the transcript is captured (redacted locally, private to you and your assistants). Capture requires a project binding and capture consent. New connections ask or use `--capture` / `--no-capture`; `capture-consent --client claude off` (or `--client codex`) in the project folder disables capture. Legacy installations with no consent file initialize existing bindings as capture-enabled; review those bindings with `aidrive-mcp-server profiles` and turn unwanted capture off explicitly. The brief shows where each recent session left off (last request, last answer, recent tools), so the next assistant can continue after a usage limit without a handoff. Codex hooks are implemented through `install-hooks --client codex` and require trust in `/hooks`; manual import remains available through `capture --client codex`. Hook implementation does not establish that every installed desktop host executed them. In the current campaign, some MCP calls registered a process fallback chat rather than the native thread; re-check the chat ID after reconnecting. Genuine idle Codex wake remains unverified and unsupported by the current integration; no polling schedule is part of this feature.

## Start the web app

```powershell
cd apps/web
npm run dev
```

The web app signs in the owner and, per space, shows text files with versions and restore, AI access (keys and pairing), handoffs, project knowledge with the review inbox, captured sessions with visibility, and activity.

To make the API status indicator work across the browser origin, start the API before the web app. The Next.js development server proxies `/api/*` to `AIDRIVE_WEB_API_URL` (default `http://127.0.0.1:8080`).

## Status

See [ROADMAP.md](../ROADMAP.md) for the first delivery sequence. Product boundaries and security assumptions are in [docs/PRODUCT.md](PRODUCT.md) and [docs/THREAT_MODEL.md](THREAT_MODEL.md).
