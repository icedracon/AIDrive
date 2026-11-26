# Release 0.1.0 readiness — 2026-11-23 (re-checked 2026-11-24)

## MCP Registry preparation — 2026-11-26

Checked the official [package types](https://github.com/modelcontextprotocol/registry/blob/main/docs/modelcontextprotocol-io/package-types.mdx), [registry requirements](https://github.com/modelcontextprotocol/registry/blob/main/docs/reference/server-json/official-registry-requirements.md) and [2025-12-11 schema](https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json). Cargo is supported directly through crates.io; its ownership marker must be visible in the rendered package README. Compared native Cargo distribution with an MCPB prebuilt bundle: prepared Cargo metadata for the existing crate, avoiding a wrapper; the tradeoff is that users still need Rust and a separately running AIDrive API. Toolchain-free installers remain a separate release deliverable.

Root `server.json` uses the current crate version `0.0.1`, explicit `serve`, API URL, workspace ID and a secret agent-key input with no credential value. Validated against the downloaded official JSON schema and checked crate name/version and visible README marker. `cargo package --locked --allow-dirty -p aidrive-mcp-server` passed packaging and build verification; inspected the resulting archive's README marker. This is not publication, registry namespace ownership verification or a complete clean-machine install. Before publication, select the release version, synchronize crate and metadata, publish the crate with owner authorization, then authenticate and publish via mcp-publisher. No registry or crates.io publication was performed.

Glama source-server submission was accepted for review in the real authenticated browser on 2026-11-26. The receipt is submission only; approval, listing URL and automated Docker-based safety/quality checks are pending. Add a backlink only once the real listing exists.

## Public CI follow-up — 2026-11-26

Public initial commit `bf9bd89` failed Rust formatting in run `45820474095`; database, MSRV and web jobs passed. Reproduced locally: user-path normalization grew two source lines from 99 to 101 characters. Full local unit tests exposed two more fixture transformations: distinct watcher paths collapsed to the same normalized username, and a Windows lowercase-path expectation retained uppercase `USER`. Restored distinct profile folders and corrected case expectations; runtime behavior is unchanged.

The exporter now formats only normalized Rust copies using the workspace edition, before creating output, and records final bytes in its hash manifest. Missing or failing rustfmt refuses export. Frozen evaluation transformations remain refused. Publication audit and snapshot regression tests now run in CI before Rust checks. Local validation: formatting, clippy, 118 Rust unit tests passed (27 integration tests ignored); three snapshot and six publication-audit tests passed. These checks do not replace live database or clean-machine package tests.

## Public snapshot preparation — 2026-11-26

Selected-chat transcript implementation (2026-11-26, pending final integration): the workspace console now separates native conversation capture from AIDrive coordination messages. `GET /workspaces/{workspace}/chats/{chat}/transcript` resolves the exact workspace/client/native session ID (Claude `claude` maps to capture `claude-code`) under existing session visibility RLS, without guessing by folder, agent or recency. Pages use `(ordinal, event_key)` cursors, including tied ordinals. This reuses the progressive disclosure and capture/privacy decisions researched in [session-memory.md](research/session-memory.md) on 2026-11-20; the tradeoff is explicit page loading and manual refresh rather than an unbounded transcript download. Private captures remain private even to other workspace members.

Observed validation: API clippy passed; live private PostgreSQL session-flow and visibility-flow tests passed, covering exact provider matching, human and assistant reads, pagination/tied ordinals, outsider rejection, private member rejection and creator sharing. The real browser against a separate synthetic database loaded 100 then all105 user/assistant events in order; the AIDrive message view remained separate. Web lint, ten existing tests and production build passed. These checks do not establish lossless capture of original native chats: capture still redacts and caps individual event text at4,000 characters, reasoning/system instructions are excluded, and missing/inaccessible captures are explicitly shown. No claim of instant model routing or idle Codex wake follows from a transcript reader.

The production preview was then connected to the existing development database through a separate loopback API, leaving the original8080 API and3000 web processes running. Browser selection of AIDrive launch → Codex → chat `fc8301c3-fb34-47bd-9735-1f7f2772785c` resolved its real capture and loaded all779 currently stored events:45 user turns,211 assistant turns and523 tool events. No document horizontal overflow was observed at the current desktop viewport. This validates the real selected-chat capture data path, not lossless original text or the final installable package. Read-only independent source review found no P0–P2 issues in the scoped transcript patch.

The owner selected a clean initial public snapshot with a GitHub noreply author, preserving the complete original Git history locally. The repository owner/name and resulting noreply identity are still unresolved. No repository was created, no public initial commit was made, and nothing was pushed or tagged. The older readiness sections below are historical reports, not validation of the final integrated candidate.

Read-only baseline audit at `f4f5354`: all local refs reached 750 blobs (744 text, six binary). Credential-pattern candidates numbered533 and collapsed to14 fingerprints; inspected contexts were synthetic redaction fixtures, URL comments/templates, isolated test URLs and documented local-development examples. No live provider credential was confirmed within this bounded pattern pass. This is not proof of absence: unknown secret shapes/private prose require review. Git author/committer metadata contained a personal email, and the deleted `ADVISER.md` remained in history; publishing the old history would expose them. Current documentation and synthetic reports also include absolute personal paths. Four sampled frames of the 809-frame terminal demo showed synthetic content; this was not an exhaustive visual audit.

`python -I -B scripts/audit-publication.py --report <new-report.json>` reproducibly scans all reachable local-ref blobs, emitting paths, line numbers and fingerprints rather than matched values. Six temporary-repository tests cover deleted historical secrets, side refs, current paths, excluded untracked files, sensitive paths, binary candidates, non-disclosure and report overwrite refusal. No external credential validation is attempted.

`python -I -B scripts/prepare-public-snapshot.py --revision <candidate-sha> --output <new-directory>` exports committed regular files only, without Git history or untracked files. It normalizes personal user-path components in text copies, records original/export SHA256 in `PUBLIC-SNAPSHOT-MANIFEST.json`, and refuses transformations of frozen evaluation manifests/data/code. Redacted evaluation outputs are labelled transformed copies rather than unchanged raw evidence. Links, unsafe paths and existing output directories are refused. Two temporary-repository tests cover source preservation, history/untracked exclusion, normalization, manifest hashes, existing-directory refusal and frozen-file refusal.

Nonfinal review export `633c290` contains245 source files and nine transformed paths. The source and exported copies both passed the six audit tests and two snapshot tests. The first review export preserved all nine frozen dataset/code digests; repeat those checks after the final integration. No runtime product validation follows from these packaging tests. Security/native changes are still awaiting the coordinated final-candidate freeze; regenerate and review the export after that freeze rather than publishing this intermediate copy. Clean-machine installation, real packaged handoff/capture and genuine idle Codex push remain separate open gates.

Report only. Nothing has been pushed, tagged or published, and no crates.io or GitHub token was used.

## crates.io

Crate names `aidrive-mcp-server`, `aidrive-api`, `aidrive-db` and `aidrive` were free on crates.io on 2026-11-23 (read-only lookup; 404 for each).

Metadata (inherited from `[workspace.package]`): `license = "Apache-2.0 OR MIT"`, `authors`, `repository`/`homepage` (`https://github.com/icedracon/AIDrive`), plus per-crate `description`, `readme = "../../README.md"`, `keywords`, `categories`. `rust-version = "1.92"` (required by `postgresql_embedded` 0.20.2).

`cargo publish --dry-run --allow-dirty`, bottom-up as they would be published:

| Crate | Result | Blockers |
|---|---|---|
| aidrive-mcp-server | **passes** (10 files, 73 KiB compressed after rebase onto `c4bc3c8`; verify build ok) | `repository` placeholder; version still 0.0.1 |
| aidrive-api | **fails verification**: `sqlx::migrate!("../../migrations")` points outside the crate, so the packaged crate has no migrations | Move the canonical migrations into `apps/api/migrations` (and point `db-tool`/scripts there), or copy them in before packaging with a check that both copies match. `eval_tests.rs` also `include_str!`s `../../../eval/dev/tutorbook.json` (tests only, not part of the verify build). Binary size: PostgreSQL is bundled at build time (49 MB archive in `OUT_DIR`), so `cargo install aidrive-api` downloads it from GitHub during the user's build; the published crate itself stays small (92 KiB compressed). |
| aidrive-db | passes, but **not useful published**: it reads `migrations/` and `scripts/` from a repository path at run time | Mark `publish = false`, or fold its migration path into `aidrive-api` |

No path dependencies between the crates; no `sqlx` offline data is needed (no `query!` macros; `migrate!` reads files at compile time).

Packaged-file audit (`cargo package --list` and the extracted `.crate` contents): no `.env*`, no `*.pw`, no local paths (`C:\Users`, `/home/USER`), no lab addresses or test-lab passwords, no real keys. The only key-shaped strings are synthetic fixtures in `capture.rs` redaction tests (`sk-abcdefghijklmnop1234`, a dummy OpenSSH header).

## Licenses and advisories

`deny.toml` added. `cargo deny check`: advisories ok, bans ok, licenses ok, sources ok.

- Allowed licenses: Apache-2.0 (also WITH LLVM-exception), MIT, MIT-0, BSD-2/3-Clause, ISC, 0BSD, BSL-1.0, CC0-1.0, CDLA-Permissive-2.0 (webpki roots), PostgreSQL (`postgresql_embedded`), Unicode-3.0, Unlicense, Zlib. `r-efi` is `MIT OR Apache-2.0 OR LGPL-2.1-or-later` and is accepted through MIT; LGPL is not on the list.
- `url` updated 2.5.8 → 2.5.9 for RUSTSEC-2026-0592 (stack overflow in `Url::origin()`); it was also present on `main`.
- Ignored with reason: RUSTSEC-2026-0543 (`chrono` unmaintained, only through `rmcp`/`schemars`, no safe upgrade).
- The bundled PostgreSQL binaries are under the PostgreSQL License; the GitHub release archives should carry that notice (not yet added).

## GitHub release

`.github/workflows/release.yml` (not run): on a `v*` tag, builds `aidrive-api` and `aidrive-mcp-server` in release mode for x86_64/aarch64 Linux, x86_64 Windows, aarch64/x86_64 macOS, packages them with README, both licenses and THIRD-PARTY-NOTICES.md (PostgreSQL and theseus notices), adds SHA-256 files and creates a **draft** release.

After T1's review (2026-11-24): the workflow default is `contents: read`; the build jobs (which run every dependency's `build.rs`) get only a read-only token on the Build step, for the PostgreSQL download rate limit; `contents: write` exists only in the `release` job, which runs no build. No cache in release builds. Still open: actions pinned by tag, not commit SHA (pinning needs the SHAs looked up on GitHub); no build provenance attestation; the PostgreSQL archive is verified by a checksum from the same theseus release (integrity, not authenticity) and is not yet pinned per target; archives do not include the web app.

## Before tagging 0.1.0 (owner decisions)

1. Repository created: `https://github.com/icedracon/AIDrive` (set in `Cargo.toml`).
2. Fix the aidrive-api migrations path (above) if crates.io is wanted; GitHub binaries do not need it.
3. Bump the three crate versions to 0.1.0 (`version` in each `Cargo.toml`; the MCP server also reports its own `serverInfo` version).
4. MCP review finding 1 (forged brief sections) is fixed on `main` by T1 (`one_line`); this branch also flattens session-search labels and snippets. `authors` in `Cargo.toml` is the owner's real name: confirm before publishing.
5. Push, tag, check the draft release on each OS, then publish.

## Owner pilot acceptance and capacity handoff — 2026-11-26

Current verdict: NOT READY to send an installable pilot package. The owner explicitly chose preparation only on2026-11-26; do not change Windows Sandbox/VM state for this campaign yet. A development-browser pass does not satisfy clean Windows installation or native idle dispatch.

### Required package proof

Freeze one integrated source commit, Windows ZIP SHA256 and dependency/package manifest. Record clean Windows version, install start/end/duration, permissions/prompts, executable version/hash and readiness. Install the exact ZIP without a GitHub sign-in prerequisite, connect actual Claude Code and Codex chats to the same dedicated test project, and record native session IDs separately from AIDrive chat IDs. Use a synthetic project with realistic continuing work; never copy a real private project into an external model/test service. Preserve client and API versions in the evidence.

Run one actual source-linked project handoff A→B, have B cite the exact source version and continue the work, edit that source, and verify B sees staleness before relying on it. Reject a write using the old version, preserving the newer content. Restart the packaged API/watcher and clients as appropriate; B must recover the same task/source history without creating a replacement chat. Capture timestamps, artifact/version IDs, errors and elapsed time for each step. None of this combined final-package journey is passed by current isolated checks.

### Dispatch evidence and failure matrix

Use one explicitly selected existing native test chat; no thread/start on retries, no substitute CLI agent and no scheduled poll as an idle-wake proof. For each message persist request_id/message_id, workspace, assistant and exact native/AIDrive destination, then correlate: stored → dispatch attempt → native turn/started → addressed inbox receipt → reply persisted → native turn/completed. A hook/inbox claim alone proves collection, not a model turn, a wake or an answer. Show unknown/unconfirmed when a host event is unavailable; only an actual reply from the selected chat linked to the original message confirms an answer. Retries need deduplication, bounded backoff and a durable dispatch attempt/ack ledger, not a new chat.

| Case | Required result | Current acceptance |
| --- | --- | --- |
| Exact idle chat | Native host starts that chat's turn and a correlated answer returns without owner prompt | OPEN; current Codex connection reports idle_push_supported=false |
| Offline assistant | Remains queued/offline or unknown; no delivered/started claim, no new chat; bounded retry | OPEN |
| Expired/revoked key | Explicit auth failure; no claim, no retry storm or switch to another key; owner reconnect required | OPEN for packaged host flow |
| Stale capture/registration | Show observed timestamp and stale/unknown state; don't infer presence from last_seen | OPEN |
| Restart during dispatch | Recover same message/target; replay cannot create duplicate task ownership or a second answer | OPEN |
| Host starts then dies before ack | Reconcile native turn identity before redispatch; uncertain state remains visible | OPEN |
| Malicious file/message | Treat content as untrusted data; no key disclosure, scope expansion or cross-workspace source access | OPEN for final package/model pair |
| Shared-account quota exhaustion | No ping-pong redistribution among chats on the same exhausted quota pool | OPEN |

The messages UI was corrected to call existing delivery rows inbox collection receipts and explicitly state that turn start/reply are not confirmed. The stored wake boolean is a caller-supplied claim and is not displayed as proof that a chat woke.

### Duplicate-chat finding and containment

Native app inventory exposed at least20 chats titled `Reply with exactly the word READY and nothing else.` in one synthetic e2e folder. Inspection found unconditional thread/start in the local probe host.py(line61) and e2e host2.py(line90). These are external scratch experiments, not a located production dispatcher path. No matching running Python harness was observed; two running app-server processes belonged to desktop hosts and were left untouched. The AIDrive heartbeat configuration found on disk was PAUSED. No chats were deleted or archived. The responsible existing push-work chat and main Claude were instructed to stop fresh-chat tests and require an explicit existing test target. A confirmation/patch receipt is still required; absence of a running harness is not proof it cannot recur. Tests should run in an isolated client data directory and retain one named test conversation instead of polluting the owner's sidebar.

### Capacity reporting and accepted task transfer (design, not implemented)

Capacity belongs to a provider/account quota pool as well as to a chat: moving a task to another chat on the same plan may not gain capacity. Publish only authorized observations: available/limited/exhausted/unknown, provider/client, quota-pool identifier without credentials, observed_at, valid_until, reset_at when actually provided, and evidence_kind(provider event, host telemetry or explicitly labelled assistant self-report). Missing or expired evidence becomes unknown; do not invent percentages, reset times or infer remaining quota from the model's confidence. Receiving a fresh rate-limit error blocks dispatch to that pool until its actual reset or updated evidence, with owner-visible reconnect instructions for authentication failures. Predicting task duration is a labelled estimate, never a measured remaining allowance.

Before a likely cutoff, the source assistant checkpoints the exact task ID, completed work, remaining next action, branch/commit, uncommitted-file state, source versions, validation results and blockers. It offers that checkpoint to one authorized recipient with recent capacity evidence. Ownership remains with the source until the recipient ACKs the exact task/checkpoint revision using an idempotent transfer ID; rejection/timeout leaves the task visibly unassigned or with the source. An accepted transfer atomically changes the ownership revision so two assistants cannot both claim it after retry/restart. Recovery uses the same transfer ID; cancellation/reassignment requires the latest ownership revision. The recipient rechecks source freshness and permissions before continuing. If all quota pools are exhausted or unknown, save the checkpoint and notify the owner; do not create new chats, switch credentials, enable paid overflow or schedule a wake silently.

Required tests before claiming capacity handoff: exhausted A→available B accepted once; B shares A's exhausted quota pool and is rejected; stale capacity becomes unknown; A crashes before ACK; B ACKs then crashes; duplicate ACK; late ACK after reassignment; recipient loses workspace access; source changes between offer and acceptance. Each result needs actual persistent task ownership and host evidence. Current code search found no provider quota telemetry/task-transfer implementation, so this remains a release backlog, not a shipped feature.
Containment recheck: independently read both replacement scratch scripts and invoked each with matching synthetic target/request UUIDs using python -I -B. Both exited2 with the explicit no-process/no-message refusal. The patch contains no app-server or transport launch path. Web lint,10 existing tests and production TypeScript/build passed for the receipt-label change. No final Windows ZIP was found by the bounded repository-tree filename search; no package hash has been frozen.
Actual browser verification on the real project Conversations page: four visible inbox collection receipts used the corrected wording; zero visible delivery labels claimed woke the chat. This verifies the present label, not native dispatch.

### Block 6 developer flow evidence — 2026-11-26

The two live assistants completed a synthetic source-linked handoff in Agent test lab: Claude created file88134daf-ae34-4fbc-89d7-5f16bb13ed76/v1cb754e0f… and handoff0dc636d9…; Codex prechecked/read it and continued to v2 4af7aa35…. Both assistants observed old-v1 update rejection with MCP isError=true and409/version_conflict. Codex independently observed HTTP409 and unchanged v2 content. Claude observed the exact cited source as CHANGED SINCE. A fresh Codex MCP process recovered that handoff, read the new source after the stale warning and continued to v3 f8a47cff…. Exact coordination messages: f5d34a57…,925fa68f…,79b52ceb… in AIDrive launch.

Scope: these operations used explicitly disclosed profile-scoped stdio helpers because Claude's own connected MCP resolved to the wrong project; Codex's existing connector remained in launch. Claude reported its existing chat received the question through its hook after a tool call. This is active-chat delivery and real model participation, not native idle Codex wake, clean Windows, final ZIP installation, full host/API restart or a repaired connected-tool route. The configured installed MCP executable was missing during this pass; a named versioned executable was used and the deployment owner was notified. The strict package/dispatch gates above remain open.
