# Pre-release security review, November 2026

Branch `claude/security-review-ff7412`, rebased on local main `eff2281` (2026-11-26). Reviewed: RLS and SECURITY DEFINER functions (migrations 0001–0024), API auth and local trust, the embedded PostgreSQL, MCP path handling, profiles and bindings, capture redaction, the release workflow, chat push (0022/0023, inbox hooks), the OpenCode client, the MSIX settings lookup (`6a64256`), and branch `claude/ws-summary-22f4d6` (`0e8f193`).

Every finding below was reproduced before it was fixed, and each fix has a test that failed before the fix.

**Readiness: no open high finding.** All three high findings (F1, F2, N3) are fixed on this branch. The open items are low.

## Findings

| # | Severity | Finding | Status |
|---|---|---|---|
| F1 | **High** | A LAN client gets the owner's session through the web app's `/api` proxy | Fixed |
| F2 | **High** | Capture redaction misses the owner device key and other secrets | Fixed (labelled and shaped secrets; see residual) |
| N3 | **High** | Any MSIX app can supply AIDrive settings and the OpenCode command | Fixed |
| N1 | Medium | Another person's key takes over a chat and its messages | Fixed |
| N2 | Medium | The automatic-wake loop guard is bypassed by starting new threads | Fixed |
| F3 | Medium | Registered-file text forges sections of the project brief | Fixed |
| F4 | Medium | The release job hands its write token to a third-party action; build scripts can read the checkout token | Fixed (see L1) |
| L1 | Low | First-party actions pinned by tag, not commit SHA; PostgreSQL binaries fetched at build time | Open |
| L2 | Low | Local trust trusts every OS account on the computer | Accepted (single-person install) |
| L3 | Low | Windows: embedded PostgreSQL secrets rely on inherited folder ACLs | Open |
| L4 | **High** (raised by pilot review) | Any local program becomes the owner: `/local/session` trusts forgeable headers and `/agent/local/claim` needs no secret | Fixed (P1) |
| L5 | Low | Inbox hooks also run in folders with no binding, using the client's default profile | Open |
| L6 | Info | Codex call notes are keyed by tool and arguments | Open (informational) |
| P2 | Medium | An assistant's `constraint` is auto-accepted and shown to every assistant under "Rules (must follow)" | Fixed |
| P3 | Medium | `bind_folder` consent is a field the assistant fills in itself | Open |

### P1 / L4 (High): any local program becomes the owner

Found by the second pilot review (2026-11), which raised L4.

- **Repro.** With the packaged install (`AIDRIVE_LOCAL_TRUST=1`), `curl -X POST http://127.0.0.1:7480/local/session -H "Origin: http://localhost:7480"` returned an owner session cookie: without `Sec-Fetch-Site` a loopback `Origin` was enough. A request to `/agent/local/claim` with no `Origin` registered an owner device key with no secret at all. An assistant that runs shell commands could make itself owner and so defeat the roles and the rule that no one approves their own work.
- **Fix.** Headers cannot tell a browser from a local program, so owner rights now rest on secrets in the user's AIDrive folders. The API writes a fresh `local.key` (this user only) on every start, and `/agent/local/claim` requires it in `X-AIDrive-Local-Key`. `/local/session` is removed: the web app gets an owner session only through the one-time link a device key mints (`aidrive open`), which goes straight to the browser and is printed only with `--no-browser`.
- **Residual.** A program running as the same OS user that deliberately reads `local.key` or `device.json`, or runs `aidrive open --no-browser`, can still act as the owner; an assistant with unrestricted shell access as you can do what you can. Keep each assistant's sandbox or approval prompts on. This narrows L2 but does not remove it.
- **Test.** `local_trust_flow_against_live_database` (claim without the key is refused; forged web-page headers get no session), `claiming_needs_the_local_key`.

### P2 (Medium): assistant rules skip review

- **Repro.** In a space that reviews by exception, `propose_item(kind => 'constraint')` with current evidence was auto-accepted and appeared in every brief under "Rules (must follow)".
- **Fix.** Migration 0030: `auto_accept_item` treats `constraint` like `keystone`; it waits for the owner. Constraints accepted before 0030 stay; the owner can undo them under Review.
- **Test.** `db/tests/knowledge_review.sql` E07.

### F1 (High): a LAN client gets the owner's session through the web proxy

`npm run dev` and `npm run start` (Next.js) listen on every interface, and the README and INSTALL tell users to start the web app that way. The `/api` rewrite proxies to the loopback API. On the way it sets `Host` to the API's own address (`127.0.0.1:8080`) and passes the caller's `Host` header on as `X-Forwarded-Host`. The caller controls that header.

- **Repro.** `next dev` was pointed at a header echo. This was sent to the machine's non-loopback address: `curl -X POST http://192.168.X.X:3999/api/local/session -H "Host: localhost:3000" -H "Origin: http://localhost:3000"`. The API received `host: 127.0.0.1:18080`, `x-forwarded-host: localhost:3000` and `origin: http://localhost:3000`.
- **Impact.** Every check in `require_local_browser` passes on those headers, so `/local/session` returns the owner's session cookie to any computer on the network. The same proxy also reaches `/agent/local/claim`, which a client without an `Origin` header passes, and which registers an owner device key.
- **Fix.** `dev` and `start` now bind to `127.0.0.1`. `require_local_caller` now refuses any request carrying `X-Forwarded-Host`, `X-Forwarded-For` or `Forwarded`. The CLI always calls the API directly, never through the proxy.
- **Verified.** After the fix, the LAN address refuses the connection (curl exit 7) and `localhost` still works.
- **Tests.** `local::tests::proxied_requests_are_not_local_commands`; `apps/web/tests/security.test.mjs` ("the web app listens on loopback only").
- **Hosted installs** that want another interface must pass their own `-H`, behind TLS, with local trust off.

### F2 (High): capture redaction gaps

A probe test showed these reaching AIDrive unredacted:

- the owner device key `aidrive_device_…`, which can mint keys and web sessions;
- `DATABASE_URL=postgres://user:pass@host`;
- `password = value` written with spaces around `=`;
- `SECRET_KEY_BASE=…`;
- `curl -u user:pass`;
- Hugging Face, Google OAuth (`GOCSPX-`, `ya29.`), Slack app, PyPI, Shopify, GitLab trigger, DigitalOcean, webhook-secret and age keys.

- **Fix.** New token prefixes; key-name suffixes; URL credentials after `KEY=`; a label followed by a lone `=` or `:`; `-u` / `--user`.
- **Test.** `capture::tests::redaction_covers_device_keys_env_urls_and_spaced_labels`, plus new false-positive guards (`git push -u origin main`, `let a = b; x : y`).
- **Residual.** An unlabelled secret with no known shape is still not caught; this is documented in `redact()`.

### N3 (High): any MSIX app can supply AIDrive settings and the OpenCode command

`6a64256` reads profiles, bindings and capture consent from `%LOCALAPPDATA%\Packages\*\LocalCache\Roaming\AIDrive\profiles` for **every** package. `opencode::outside_path` takes any package's `LocalCache\Local\…` copy with the same size and modification time as the command OpenCode runs. Any MSIX app can write its own LocalCache, including a sandboxed (AppContainer) Store app.

- **Impact.** A planted `bindings.json` plus `capture-consent.json` plus a profile with its own `api_url` sends the transcripts of a folder to a server of the app's choosing. A planted binary becomes a full-trust command run by OpenCode.
- **Repro.** `opencode::tests::only_claude_desktops_package_can_supply_the_command` failed before the fix: an `Evil.StoreApp_8wekyb3d8bbwe` copy was returned as the command.
- **Fix.** Both lookups now go through `connect::claude_package_dirs`, which accepts only package families ending in Anthropic's publisher id `_pzs8sxrjxfjjc`. The publisher id comes from the signing certificate.
- **Tests.** The repro test above, and `connect::tests::only_packages_signed_by_anthropic_supply_settings`.

### N1 (Medium): another person's key takes over a chat

`app.upsert_chat` (0022/0023) moves an existing chat to whichever key registers the same client and session id. Session ids are listed to every viewer by `workspace_chats`.

- **Impact.** In a space with two owners, one owner's key can redirect the other's chat, and the messages addressed to it, to itself.
- **Repro.** `db/tests/security_review.sql` SR01 failed before the fix.
- **Fix.** Migration `0025_security_review.sql`. A chat moves only to another key of the same person, which covers a folder rebound to a new profile (SR02), or away from a revoked or expired key. Otherwise registration is refused with `AD409`.

### N2 (Medium): the loop guard is bypassed by starting new threads

The guard allows 6 automatic wakes per thread since a person last wrote in that thread. Two assistants that always start a new thread, for example because an injected message told them to, wake each other without limit. Only the 30-second rate limit applies.

- **Repro.** SR03 recorded 20 automatic wakes into one chat across 20 threads.
- **Fix (0025).** A chat is also woken at most 12 times, across all threads, until a person writes anywhere in the space. Held messages arrive on the chat's next turn (SR04). The owner is told once (SR03). A message from a person lifts the pause (SR05).
- **What was already right.** Same-space-only delivery: chat foreign keys are `(workspace_id, id)`, and claims check the chat's key. Hook framing: bodies are quoted with `> `, subjects are flattened, and the text says it is data, never an instruction. Hook commands quote the executable and pass fixed arguments.

### F3 (Medium): registered-file text forges brief sections

The brief flattens team-written text with `one_line` so that it cannot start a section such as `## Rules (must follow)`. The "Registered files past keep-until" section did not do this: `path` and `purpose` are written by any member or key of the space, and they went in raw. That section also told the assistant to delete files someone else had registered.

- **Fix.** Every field goes through `one_line`. Each line names who registered the file, and says to check with that person before deleting a file someone else registered.
- **Test.** `index::tests::due_files_cannot_forge_brief_sections`.

### F4 (Medium): release workflow supply chain

`softprops/action-gh-release@v2`, a third-party action on a mutable tag, ran with `contents: write`. `dtolnay/rust-toolchain@stable` installed the compiler that builds the release binaries. `actions/checkout` left the token in `.git/config`, where every dependency's build script could read it.

- **Fix.** The release job drafts the release with the preinstalled `gh` CLI. The toolchain comes from `rustup`. Checkout runs with `persist-credentials: false`. Only first-party `actions/*` remain.
- **Test.** `apps/web/tests/security.test.mjs` ("release workflow uses first-party actions only").

### Low and informational

- **L1.** `actions/checkout`, `upload-artifact` and `download-artifact` are pinned by tag. Recommendation: pin them to commit SHAs (needs a network lookup, not done here) and add build provenance (`actions/attest-build-provenance`). `postgresql_embedded` downloads PostgreSQL at build time and checks it against a checksum from the same release. Pin a known hash when the crate allows it.
- **L2.** With local trust, any process of any OS account that can reach loopback can claim an owner device or open the owner's web session. This is the documented design of a single-person install (0014, 0019). Pilots on shared computers should not enable `AIDRIVE_LOCAL_TRUST`.
- **L3.** On Windows, `superuser.pw`, `app.pw` and the data folder get only the ACLs inherited from `%LOCALAPPDATA%`, which is per-user by default. A shared `AIDRIVE_DATA_DIR` is not hardened. Unix sets 0600/0700.
- **L4.** Without `Sec-Fetch-Site` (older browsers), `require_local_browser` accepts any loopback `Origin`, including a page on another localhost port.
- **L5.** By code reading: `inbox::target` resolves the client's default profile for folders without a binding. Messages can therefore wake chats in unrelated projects. Delivered text is framed as data, but scoping the hooks to bound folders would narrow the injection surface.
- **L6.** In Codex, one process hosts every thread. Two threads making the identical tool call within 10 s can be stamped with the wrong `from_chat`. Integrity only; no cross-key effect.

## Checked and found sound

- `app.has_role` is pinned to `app.workspace_id`, so membership in another space gives nothing; every policy and definer read goes through it.
- Roles: `agent_rank` drops any key above low to low once its issuer stops owning the space. `require_role` stops medium keys from accepting their own records. Grand keys reach only project-state functions, never keys, members or pairing. Local `connect` issues low keys only (`LOCAL_SCOPES`).
- Private visibility: sessions, events, knowledge and evidence go through `can_see`. Evidence may cite only sessions the proposer can see. The activity feed filters private knowledge.
- CSRF: the global middleware requires `Sec-Fetch-Site: same-origin`, or an exact `Origin` match, on every state-changing route outside `/agent/*`. `/agent/*` accepts only bearer tokens. The cookie is `__Host-`, `Secure`, `HttpOnly`, `SameSite=Strict`.
- One-time login: 256-bit code, stored only as a hash, single use, expires after 2 minutes.
- Embedded PostgreSQL: listens on 127.0.0.1 only, uses scram-sha-256, and the app connects as the restricted `aidrive_app` role. On Unix the socket folder is private (0700). On Windows it adopts a running server only if it is the postmaster recorded in our own data folder.
- OpenCode config merge: comment-preserving, backed up first, re-parsed and checked after merging, and it writes no token.
- The committed eval outputs (`eval/agents/runs`) contain no keys.

## Branch `claude/ws-summary-22f4d6` (`0e8f193`)

No security finding.

- Every count runs under RLS with `app.workspace_id` set to the space being counted, switched per space in the batch. The activity time comes from `app.workspace_activity`, so other people's private knowledge does not move it.
- Unknown spaces, foreign spaces and non-UUID ids all return the same 404.
- Nit: `summary()` selects the workspace with no `WHERE w.id = $1` and relies on the RLS policy to return only that row. An explicit filter would keep it correct if a policy ever changes.

## Validation (2026-11-26, this branch)

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`: clean.
- `cargo test --workspace`: API 35 passed (22 live tests ignored here and run by api-check), MCP 67 passed.
- Web: `npm run lint`, `tsc --noEmit`, `npm test` (10 passed) and `npm run build` all pass. `@phosphor-icons/react` 2.1.10 was copied from another worktree because this one predated main's new dependency.
- `scripts/db-check.sh` and `scripts/api-check.sh` were run in WSL Ubuntu against PostgreSQL 17. Both print their ALL … PASSED lines and exit 0, including SR01–SR05.
