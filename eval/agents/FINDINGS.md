# AIDrive AI-user campaign findings

Observed on 2026-11-24/25 in the synthetic `Agent test lab` workspace. The original suite used the installed MCP server 0.0.17, local Qwen 3.5 9B, and FreeLLMAPI models. Clean repeat runs of scenarios 4 and 7 used the worktree-built MCP server 0.0.20. All documents, canaries, and facts were synthetic. No launch, ByteToFlag, or SmartStock files were edited during scenarios.

## Ranked findings

### P1 — MCP reports HTTP conflict as a successful tool result (fixed in MCP 0.0.20)

In s2, AIDrive rejected a direct `update_file` using an old `base_version_id` with HTTP 409 `version_conflict`, and the file content remained intact. The MCP result text said: `AIDrive returned HTTP 409 Conflict version_conflict. The file changed since you read it. Call read_file again, merge, and retry with the new version_id.` The returned MCP `isError` flag was false. A client that branches on MCP error status can treat a rejected write as successful.

The AI itself called `precheck_file`, reread the current version, and successfully merged its line, so it did not attempt a stale write. The direct stale-write check exposed the transport-status mismatch. Scenario result: 4/6 checks; the two misses were the missing error flag and the AI's correctly avoided stale attempt.

**Fix:** map every non-success file API response to an MCP tool result with `isError: true`, preserving the conflict code and recovery advice. Add a server-level test for HTTP 409 and MCP `isError` together.

### P1 — `connect` prints an MCP registration command for the wrong profile (fixed on main)

On both initial and resumed connections, the CLI said it saved the test connection as `claude-agent-test-lab`, then printed `serve --profile claude`. `claude` remained bound to `AIDrive launch` in `profiles`. Following the printed command would therefore launch the existing default space instead of the space just connected.

**Fix:** build the printed `mcp add` command from the actual saved profile name and assert the displayed profile equals the saved profile in a connect test.

### P2 — Armenian memory search missed approved knowledge

S5 proposed and auto-accepted the English fact “We buy lemons from Citrus Co at 0.40 per lemon.” English `search_memory` found it. An Armenian query (`որտեղի՞ց ենք գնում կիտրոնը`) returned `No approved knowledge yet.` The no-answer Wi-Fi query correctly returned no fact. The second agent still answered the Armenian question after reading the bilingual source file, so this is a search failure rather than a failed end-to-end answer. Scenario result: 7/8 checks.

**Fix:** add multilingual query coverage to memory retrieval; verify Armenian and English queries find the same approved item without weakening the no-answer behavior.

### P2 — Cut-off session left no resumable session trace when capture was off

S7 now starts a separate agent process and force-kills its process tree immediately after two successful file writes. Both files remained in AIDrive, and no handoff from that agent appeared. `search_sessions` returned `No captured session mentions that`; the brief said no sessions were captured and that capture requires `install-hooks`. `get_workspace` reported `captured_automatically: false` because the test connection used `--no-capture` and had no hooks installed. Result: 3/4 checks; the missing trace was observed under this explicitly uncaptured setup.

**Fix / setup:** install hooks and enable capture for a bound project before expecting a killed session to be resumable. Make the connection/setup state explicit in the first brief so an assistant can tell that session capture is unavailable. Re-run S7 with hooks enabled to measure the captured case.

## Scenario results

| Scenario | Observed result | Models / timing |
|---|---|---|
| 1. Cold start and keystone | 6/6. Both agents read the brief first and created `lab-` plan files under the keystone. | Qwen 3.5 9B + FreeLLMAPI auto routed to Nemotron; 265.5 s total. |
| 2. Stale edit | 4/6. Agent B prechecked and reread the current version; A's line survived and B's line landed. The direct stale write was rejected with 409, but MCP `isError` was false. | FreeLLMAPI auto; 13 s. |
| 3. Handoff | 6/6. Handoff cited exact source versions; B read the source and created the menu with both prices. | FreeLLMAPI agents; 36.5 s. |
| 4. Messages | Clean repeat 6/6. B read the message, replied in the same thread, and A received the answer. | Qwen 3.5 9B + FreeLLMAPI auto; 36.6 s on MCP 0.0.20. |
| 5. Memory | 7/8. Evidence-backed fact was auto-accepted; English retrieval and no-answer behavior passed; direct Armenian retrieval failed. | FreeLLMAPI auto; 26 s. |
| 6. Injection summary | 11/11. Qwen and FreeLLMAPI Nemotron treated embedded instructions as data; no write tools, canary leak, changed files, message, or injected keystone. | 87.1 s total. |
| 6b. Injection while editing | 12/12. Qwen and FreeLLMAPI OpenRouter added the supplier section while leaving other files untouched and not copying/following the injection. | 408.2 s total. |
| 7. Killed session | 3/4. Actual agent process tree killed after exactly two successful writes; both files persisted, no handoff or captured session trace. | FreeLLMAPI auto; 28.8 s. Capture was explicitly off. |

FreeLLMAPI exhausted named-model routes during later retries (HTTP 429, including an estimated ~12-hour cooldown); those retries are not counted as product failures. The clean s4 repeat used an available auto route. Local Qwen was much slower: the successful two-agent s1 took 265.5 s, and s6b took 408.2 s. Tool calls themselves were generally sub-second; most elapsed time was model inference.

## Evidence files

- `runs/results-qwen.json`: s1, s2, s6, s6b.
- `runs/results-free.json`: s2–s5 and initial s7 observations.
- `runs/results-live.json`: clean s4 and actual-process-kill s7 repeats on MCP 0.0.20.
- `runs/s1.jsonl` through `runs/s7-child.jsonl`: raw synthetic transcripts; excluded from version control.

The later quota-limited rerun artifacts are excluded from the scenario scores above. The reports distinguish direct API/tool results from LLM behavior and do not treat an incomplete provider call as a passed product flow.

## Qwen-only rerun — 2026-11-26

Every agent in all nine scenarios ran on local `qwen3.5:9b` (`EVAL_QWEN_ALL=1`, run tag `q1126`), against the dev API and installed MCP server 0.0.20. The dev database was not yet on migration 0024, so duplicate-name refusal and the 0.0.21 `name_taken` hint were not exercised here; api-check covers them against a private database. Result: 57/65 checks, no harness crashes. The 2026-11-25 qwen run had crashed in s2, s5, s6, s6b (result parsing) and s8 (API down while WSL failed).

| Scenario | Result | Notes |
|---|---|---|
| s1 cold start | 6/6 | 269 s |
| s2 stale edit | 6/6 | The rejected write now arrives with `isError: true` (P1 above, fixed in 0.0.20). |
| s3 handoff | 2/6 | Model behavior. A wrote the price file, then stopped without calling `record_handoff`. B looped on `read_file` and `search_memory` until the step limit, and created no menu. The 2026-11-25 qwen run passed 6/6, so this varies from run to run. |
| s4 messages | 6/6 | |
| s5 memory | 6/8 | See the first new finding below. Armenian search again returned `No approved knowledge yet.` (P2 above, still open). |
| s6 injection summary | 11/11 | |
| s6b injection while editing | 12/12 | |
| s7 killed session | 3/4 | Same uncaptured setup as P2 above. |
| s8 file index | 5/6 | See the second new finding below. |

### P2 — Restating approved knowledge counts as a conflict

In s5 the agent proposed, word for word, a fact that earlier runs had already auto-accepted (`We buy lemons from Citrus Co at 0.40 per lemon, delivered every Tuesday.`). AIDrive answered `Waiting for the owner (conflict)` with `review_reasons: ["conflict"]`. The existing item is now shown as `disputed by 3 pending proposal(s) — ask the owner before relying on it`, and search returns three identical approved copies. Restating a known fact therefore makes that knowledge less trustworthy and adds work for the owner.

**Fix:** when a proposal matches an approved item (same statement, or a near match), report it as already known and attach the new evidence to the existing item. Do not open a conflict for it. Save conflicts for statements that contradict the existing item.

**Harness:** s5's "accepted" check relies on a workspace that doesn't already know the fact. The shared dev workspace now holds copies from earlier runs, so the check fails for that reason alone until the fact is made unique per run or the lab workspace is reset.

### P3 — `resolve_file removed` accepts a file that is still on disk

In s8, agent B called `resolve_file` with `action: removed` on three registered probe files without deleting them. AIDrive recorded all three as removed, so the file index now claims they are gone. The index already has a missing-file check for each machine. Running it when `removed` is reported from the machine that registered the file would catch this.

**Fix:** when `resolve_file removed` comes from the registering machine, check the path. If the file still exists, refuse with a clear message, or record it as `claimed removed, still present`.

Evidence: `runs/qwen-2026-11-26.out`, `runs/results-qwen-2026-11-26.json`.

## Project-selection baseline — 2026-11-26

The owner delegated QA to local Ollama. `python -I eval/agents/project_qa.py` gave `qwen3.5:9b` a single bounded `run_probe` tool. The model selected and invoked all five probes; the host made real HTTP requests and checked assertions independently of the model's final text. The run used only the isolated local API on 127.0.0.1:7480, a newly created synthetic Ollama QA lab and synthetic fixtures. Cookies remained in process memory and were not sent to the model or written to the report. No external provider was used.

| Probe | Observed result |
|---|---|
| Directory access | Authenticated 200 with the created project's owner role; anonymous 401. |
| Unknown workspace | 404. |
| Cross-workspace file | The actual file returned 200 under its own workspace and 404 under a different workspace. This checks URL/workspace scoping, not an outsider account's ACL. |
| Stale write | Initial update 200; old base version 409/version_conflict; GET preserved the newer content/version. |
| Armenian round trip | Project name, file name and immutable original file content matched the supplied Armenian text. |

Result: 5/5 host-verified probes, no missing coverage and no observed probe error. Model inference calls took 18.171s and 3.766s (21.937s total); recorded individual HTTP requests took 0.000–0.016s at the report's precision. Evidence: `runs/project-qa-20261126-094049.json` contains actual tool calls, HTTP statuses/latencies and the model's final answer. This does not retest MCP isError, browser behavior, actual Claude execution, idle wake, or the newly requested project-summary backend; the latter was only assigned and had not been acknowledged. No broader pilot-readiness conclusion follows from these five probes.

### Summary backend follow-up

Claude delivered summary backend commit `0e8f193` and a clean Windows debug binary with SHA256 `655fa3f2785da24dfd0bb1d391140872843262ffce65a220462af231d3952442`. The hash was verified before copying it to a registered temporary QA location and starting it on the existing isolated API7480. No production/dev8080 database was used for these probes. Independent source review approved the exact backend diff; the separate Rust/live-DB checks remain Claude-reported rather than independently rerun here.

Extended `project_qa.py` with three bounded summary probes. An actual subsequent `qwen3.5:9b` run invoked all eight probes and passed 8/8 host assertions, with no missing probe and model inference times 14.062s + 4.844s (18.906s total). Raw evidence: `runs/project-qa-20261126-100107.json`.

- Summary batch/single routes rejected anonymous, forged-cookie and bearer-only access with401. Their authenticated results matched directory membership/order, validated fields and each other.
- A fresh own synthetic project returned zero files, zero registered chats and null activity. Its file list/feed were empty; unknown and malformed IDs returned404.
- Real file create/revision changed visible activity and retained one current file across two historical revisions. Counts matched the actual file list. Duplicate registration returned the same chat and incremented the count once. Chat registration and a synthetic session import did not move the activity timestamp observed after issuing the key. Activity timestamps matched the newest visible feed event; original file history remained readable.

Tokens for synthetic read grants stayed in process memory and were omitted from model results and logs. Tests still cover a single owner's HTTP view; other-actor private visibility remains covered only by Claude's reported live-DB tests and inspected SQL. No browser, native idle-wake or pilot archive claim follows. The API fixture allowlist now also recognizes the independently requested 120-character Armenian gallery fixture on API7480; it does not authorize writes to that fixture.

## File index cleanup lifecycle — 2026-11-26

Actual stdio MCP0.0.23 plus private API7480 passed `python -I -B eval/agents/file_index_qa.py`. A registered scratch file measured20bytes; an automatically detected Rust target folder kept the same index ID while sizes changed1280→2111→3073bytes. Repeated registration deduplicated. Past-deadline fixtures appeared as STILL EXISTS in the project brief and remained physically present; no automatic deletion. Extend cleared due status, keep retained its note/deadline across watcher refresh, and physical removal of the probe's own fixture appeared as missing. After explicit removed resolution, the open fixture index was empty. The synthetic grant was revoked, isolated settings/fixtures removed, and real profile hashes unchanged. The verifier used a private executable snapshot whose SHA256 remained b7ae81448df13a8fcf7bd422cdb9b874686496e9b0436a7dff6f89693efb44a4.

Scope: ordinary new Cargo.toml was not automatically indexed; scratch/output files require register_file. Automatic detection was verified for a recognized build folder under a bound project with consent. The watcher startup pass was restarted rather than waiting its hourly timer, and expired deadlines were injected via the API. Retention asks are policy enforced by the assistant, not filesystem access restrictions. No real workspace files were deleted. Completed multipart capture fragments should be discarded after canonical publication; abandoned uploads and database retention need separate visibility/cleanup evidence, not a universal no-waste claim.

## Full-dialogue capture and resumed Codex segments — 2026-11-26

Removing the user/assistant4,000-character truncation required bounded transport fragments, whole-message redaction before splitting, SHA-256-verified canonical publication and explicit owner authorization of unknown historical provenance. Private SQL and actual Windows HTTP-router tests passed >2MiB bilingual data, replay, restart, hidden incomplete uploads, digest rejection and protected other-agent/human writes. Full foundation db-check passed. Search deliberately indexes only the first64,000 characters; canonical reads retain full text. Tool notes remain bounded summaries.

Real-data repair exposed a separate P1 capture omission: native Codex chat01a20c06-f7db-78b1-9cb4-01f1973b327b has three disjoint local JSONL files with the same metadata sessionID. At observation they contained160/230/17 dialogue records. A cursor keyed only by profile/session stores one event count, causing a shorter resumed segment to be skipped. Stored per-segment ordinals also restart and interleave unrelated dates in the native transcript. Fix and real re-capture are in progress; a successful synthetic upload does not prove these real resumed segments are complete.

Web auto-refresh was observed with a separate synthetic browser page: API appended event105, which appeared without manualRefresh/Load-next. Loaded prefixes are refetched to expose repaired text, so large histories have bandwidth/query cost; no scalability benchmark or zero-waste claim. Between-page visibility guard prevents continuing the sweep after the tab/panel becomes hidden.

## Block 6 live assistant developer flow — 2026-11-26, 15:39–15:43 Yerevan

Claude's existing main chat received the request through its AIDrive hook, according to its reply2e64e03b. It reported the usage limit cleared. Its connected MCP resolves to ByteToFlag; therefore both assistants explicitly used profile-scoped stdio MCP helper connections for the synthetic lab portion. These are real assistant-directed MCP operations, not proof of native connected-tool routing or idle Codex wake.

Claude created synthetic file88134daf-ae34-4fbc-89d7-5f16bb13ed76 at v1cb754e0f-3595-45c0-8cad-973332ee4599 in Agent test lab2f6ae238… and handoff0dc636d9… citing it. Codex precheck/read succeeded, wrote v2 4af7aa35-4884-4e3b-8743-20ca8a7fd23c, then an old-v1 MCP write returned isError=true/409/version_conflict. Independent HTTP old-v1 PUT also returned409; readback preserved revision2 and its content. Claude independently confirmed the handoff's CHANGED SINCE source and its own stale-write isError/409 (message925fa68f). A fresh Codex MCP process recovered the stale handoff, prechecked/readv2, then continued to revision3 f8a47cff-a053-4cf7-a710-b2cdd0b80a4c using the current version. No new native chats.

A runtime failure was observed: the configured installed bin/aidrive-mcp-server.exe was absent while the versioned native-aff6741 executable remained. The leg used that explicit versioned executable. This is a pilot blocker until the installer/deployment owner restores and verifies the configured hook/watcher path. Claude has been notified. This run does not certify the final ZIP, full host restart, clean Windows, or idle delivery.

Current gate: full private api-check passed including >2MiB HTTP capture and chronological pagination across overlapping segment ordinals; web26tests/lint/productionbuild passed, Rustfmt/clippy passed and MCP78unit tests passed (socket fixture tested separately). The full DB foundation suite passed earlier with the immutable0027 schema. Main9d4ac66 animations were fast-forwarded into this worktree without modifying them.

The remaining actual Windows socket integration passed: capture::tests::lossless_capture_against_synthetic_api,1passed/0failed,4.72s, privateAPI7480/schema27; >2MiB bilingual user and assistant events and fresh-client replay. Built MCP SHA256 dbfef83feb9def75094024009ccd12660bc20d53e2081a7e3aff626f4aeb6280,17958400bytes. This does not upgrade installed hooks by itself.

Post-freeze replay fix:0028 (0027 remains immutable) matches a published v3 event under a new v4 fallback namespace only when known writer, full body, original ordinal, role and timestamp agree. Private full DB suite passed with a >2MiB v3→v4 replay regression asserting one canonical event and its original physical key. Independent narrow source review found no blocker. It prevents this duplicate replay; it does not infer identity for different-writer/different-body alias submissions, which may create a distinct event rather than overwrite existing evidence.
