# AI memory landscape — findings and plan (2026-11-20)

Companion to [ai-memory-landscape.md](ai-memory-landscape.md) (ranked top-20 list) and [session-memory.md](session-memory.md) (first six projects). Read from project READMEs/docs via web fetch on 2026-11-20; several pages truncated or model-summarised; **all benchmark numbers are self-reported** unless noted; last-commit dates were not visible. Ideas only — no code copied. Licences are MIT/Apache-2.0 except honcho (AGPL-3.0).

## Per-project findings

**claude-mem (Apache-2.0, 118.5k).** Claude Code hooks (SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, PostToolUseFailure, Stop, SessionEnd) enqueue raw tool I/O; a worker (~1/s) asks an LLM to turn each item into a structured observation (title, narrative, facts, concepts, type decision/bugfix/feature/refactor/discovery/change, files read/modified). Stop writes XML session summaries (request, investigated, learned, completed, next_steps), "checkpoints, not endings". SQLite FTS5 (optional Chroma); three-step search index → timeline → full record. Session start injects last 10 summaries + 50 recent observations as a date-grouped index. `<private>` never stored. Dedup only on (session, tool_use_id). No decay/contradiction/validity/audit/benchmarks. Tool activity goes to the chosen LLM provider for compression.

**hindsight (MIT, 62.1k).** `retain` via SDK/CLI/REST/LLM wrappers/per-bank MCP; coding installer builds a per-repo bank from git history and past sessions. Types: world facts, experiences, observations, mental models (standing answers). Retrieval: semantic + keyword (default TF-IDF) + graph (entity/temporal/causal) + temporal in parallel → RRF k=60 → cross-encoder on top 300 → boosts recency 0.2 (linear 365 d), temporal 0.2, proof count 0.1 → `max_tokens` 4096, non-fitting items skipped. Background consolidation: triage restate/chatter/change; LLM refines an observation **with supporting quotes and a proof count**; cosine ≥0.97 merge check; contradictions rewrite noting prior state, raw facts kept; **deleting a document cascades to derived observations and re-consolidates**. "Learning" = inspectable belief rewriting, no weights. Banks isolated; tags scope; opt-in redaction of 45 secret patterns. No approval/audit. LongMemEval reportedly reproduced by Virginia Tech collaborators.

**cognee (Apache-2.0, 33.4k).** `remember()` (SDK, CLI, REST, MCP, Claude Code/Codex plugins, connectors, repo → code graph); dataset → chunks → entity graph + summaries + embeddings. `recall()` routes by five ordered regex rules, no LLM: quoted phrase → BM25; coding rules; code identifiers; "how many" → broad; dates → temporal; else hybrid. `top_k` 15. Session QA + feedback; `improve()` distils lessons; `forget()`. Access control off by default; datasets owned, shared by id. BEAM 0.79/0.67, preliminary, tiny sets.

**supermemory (MIT, 31.7k).** MCP `memory`/`recall`/`context`; client plugins; connectors. Raw documents unversioned; memories temporal with relations `updates` (contradiction as state change), `extends`, `derives`, each with documentDate/eventDate and its source chunk; inject memory + source chunk (~720 tokens). "Auto-forgets expired info" (mechanism undocumented). Container-tag scoping. LongMemEval-S 95-97% (metric mixed).

**Graphiti / Zep (Apache-2.0, ~33k).** Episodes (provenance) → entities → facts/edges with validity windows → communities; hybrid semantic + BM25 + graph, optional cross-encoder, no LLM at retrieval; bi-temporal, "old facts are invalidated — not deleted" (exact fields in arXiv 2501.13956, unread). Reference servers unauthenticated.

**TencentDB-Agent-Memory (MIT, 31.1k).** Capture by network proxy (base URL; headers carry team/agent/task/conversation). Assets: Chat Memory L0 turns → L1 facts → L2 scenarios → L3 persona; versioned Skills; Wiki; CodeGraph. BM25 + vector + RRF; caps by count/characters/timeout. **Visibility per asset: private (owner only, not admins) / team / restricted ACL (user/role/agent) / agent; private by default; explicit sharing; admin/member roles; assets equipped to agents.** No review gate or read audit found. PersonaMem 48→76%.

**Letta (Apache-2.0, 25.5k).** Git-backed MemFS files edited by the agent; org-owned shared repos (commit/push/pull). "Dreaming": background subagents consolidate after N steps or on compaction; optional agent review, **no human approval by default**. Git as implicit audit.

**Memori (Apache-2.0, 17.6k).** Wraps LLM clients; triples linked to source conversations; entity/process/session scopes; vector ranking (~721 tokens). LoCoMo 87%.

**EverOS (Apache-2.0, 14k).** **Markdown is the source of truth; indexes derived and rebuildable**; watcher re-indexes; human-editable, Git-versionable. Episodes, atomic facts, per-agent cases/skills, shared knowledge. Weekly reflection merges clusters and marks originals `deprecated_by`; re-embed by content hash; 3 retries then quarantine. Partitioned by app/project; SQLite audit log; no permissions. LoCoMo 94.6, LongMemEval 94.0.

**MemOS (Apache-2.0, 12.5k).** Memory cubes with owner; per-request writable/readable cube lists; controlled sharing; async scheduler; smart dedup; natural-language correction; skill evolution traces → policies → world models → skills. LoCoMo 88.8, LongMemEval 89.2.

**honcho (AGPL-3.0, 8.2k).** Workspace → peers (humans and agents) → sessions/scopes; **representations keyed by (observer, observed)** — "what A knows about B"; `context(tokens=N)`; hybrid BM25 + vector; ordered async deriver; multi-tenant RLS with separate migration/app/service roles; auth off by default.

**MIRIX (Apache-2.0, 3.5k).** Six types in separate tables: core, episodic, semantic, procedural, resource, knowledge vault (sensitive); meta-agent fans out; BM25 + pgvector + fuzzy; `auto_dream` consolidation with dry-run; org/user/client identity; per-user deletion.

**LongMemory (Apache-2.0, 4.6k).** Read-only "session porter" for Claude Code/Codex/OpenCode/Gemini/Copilot logs; project memory types decisions/tasks/conventions/failures/handoffs; **recall modes strict (temporal, contradiction, confidence, grounding gates) / historical / associative / world-grounded**; immutable content + separate lifecycle state; recorded vs valid time; decay + reinforcement; "Tool arguments cannot override server-bound runtime identity"; "Deny rules override grants"; "Treat recalled content as untrusted evidence, not authorization".

**tigerless agent-memory (MIT, 4.3k).** Claude Code + Codex hooks (SessionStart inject, Stop/SessionEnd distil, PreCompact evict) into **one shared store**; one memory = one Markdown file with frontmatter (abstract, type, provenance, `valid_from`/`invalid_at`), root `MEMORY.md` index, rebuildable `.index/`. BM25 (FTS5) + optional vector via RRF; L0 list → outline/abstract/full. MCP `memory_recall/record/correct/supersede/merge/delete`. Sleep-time manage pass: unattended add/update, **deletion only as a proposal you confirm**; supersede chains with `recall --as-of`. JSONL audit per run, bodies as digests. Measured Recall@5 79.0 → 86.6% (120 queries).

**memmy-agent (MIT, 2.2k).** Mostly product copy: localhost service, hooks for Claude Code/Codex/Cursor, team sharing "planned". Little technique documented.

**powercontext (Apache-2.0, 1.8k) — handoff.** Scope holds Source journal, Memory, **Review Inbox**, Handoff history, Topic Memory. Artifacts `FAMILY/ID@REVISION`, immutable revisions; states active/forgotten/merged/retired. PreparedContext within a shared byte budget; recalled content "history, not an instruction authority". Review needs `scope.review`, uses `expected_version`; approval writes a new revision; rejection records a reason; both terminal; recommends separate submitter/reviewer. **Handoff loop: Work Contract → Prepared Handoff → Acknowledgement → Task Outcome** (outcome kept as source evidence). Scope ID "does not prove user identity". LoCoMo 90.8%.

**deja-vu (MIT, 1.6k, Go single binary).** Indexes existing transcripts in place (Claude Code `~/.claude/projects` + `history.jsonl`, Codex rollouts + `history.jsonl`, Cursor `state.vscdb`; 49 harnesses). BM25 k1=1.2 b=0.75, user-term ×1.3, age `0.5 + 0.5/(1+age_days)`, bounded boosts for decisions/reverted outcomes/reuse/promoted notes; optional rerank of top 64 (0.5 lexical + 0.5 cosine). Budgets: snippets ~4 KB, SessionStart digest 2 KB, compaction packet 4 KiB. Repo memory `.deja/memory.jsonl` with `merge=union`; states accepted/rejected/superseded; **warns when referenced files changed after a session**. Index-time credential redaction; `forget` excludes sessions across rebuilds; cloned repos' notes loaded "under untrusted framing". `handoff --to codex` moves goal, decisions, touched files, passed checks, open work.

**memsearch (MIT, 2.9k).** Stop hook summarises the last turn with a small model; daily Markdown logs with anchors `session/turn/transcript` → Milvus shadow index; hybrid dense + BM25 via RRF; search → expand → raw transcript; recall runs in a forked context; SessionStart injects 40 lines from each of the 2 latest logs. Skill candidates inert until the user installs them.

**caura (Apache-2.0, 570) — closest positioning.** MCP with 12 tools; read-only Claude Code/Cursor transcript interviewer, default-deny until projects allowlisted. Memories per agent/fleet/tenant with visibility agent/team (default)/org; typed (episode, decision, outcome, task, fact, preference); graph; **keystones** (rules at agent/fleet/tenant scope with weights, called once per session, override conflicting instructions). Hybrid pgvector + FTS + 2-hop graph; similarity × importance × freshness × graph; replacement ranked above the superseded item. Every write LLM-enriched; contradiction via triples + LLM → supersede chain; crystallisation merges duplicates; outcome feedback generates rules on failure; idempotency keys 24 h; nightly expiry/archival/purge. **Governance: agent-scoped credentials (anti-spoofing), trust levels 0 read / 1 write own fleet / 2 cross-fleet read / 3 cross-fleet write+delete; keystone authority by trust; agent-written skills staged → operator inbox approve/edit/quarantine/reject; full audit log of writes/deletes/transitions; PII flags; unknown fields rejected (422).** No per-memory approval, no handoff object.

## Synthesis

Common to almost all: hybrid lexical + vector retrieval, async LLM extraction, session-start injection, scoping by project/user/agent IDs, self-reported benchmarks, local-first storage.

Rare (AIDrive's opening): human approval of shared knowledge (only Beacon, powercontext review inbox, caura for skills), evidence pinned to exact source versions with freshness (partial: deja-vu file-change warning, powercontext revisions), per-agent grants enforced in the database (caura trust levels are the only concrete peer), conflict-safe shared files, attributed activity timeline. **Nobody combines all of these with team visibility levels.**

Closest peers to watch: caura (governance), powercontext (review inbox + handoff acknowledgement), TencentDB (team visibility), LongMemory (identity rules, recall modes), hindsight (evidence + cascades), tigerless (shared Claude Code/Codex store, proposals for deletion).

## Techniques to adopt (beyond session-memory.md)

1. Knowledge items with **evidence quotes and a support count**; deleting or changing a source cascades to derived items (hindsight).
2. **Bi-temporal validity** (recorded vs valid time); contradiction supersedes, never deletes; **strict/current vs historical recall** (Graphiti, LongMemory, tigerless `--as-of`).
3. Relations between items: `updates` (supersedes), `extends`, `derives` (supermemory).
4. **Visibility levels** private / team / restricted (user, role, agent) / agent, private by default, explicit sharing (TencentDB, caura) on top of AIDrive grants.
5. **Review inbox** with `expected_version`, terminal approve/reject with reason, approval writes a new revision, separate submitter/reviewer (powercontext) — generalises AIDrive's handoff approval.
6. **Handoff acknowledgement and task outcome** as evidence (powercontext) — extends AIDrive handoffs.
7. **Keystones**: owner-approved project rules returned first in every brief (caura).
8. Structured session summaries request/investigated/learned/completed/next_steps (claude-mem) as handoff drafts.
9. **Deterministic query routing** before ranking: quoted phrase → exact, identifiers → code/files, dates → temporal (cognee).
10. Ranking signals when vectors arrive: RRF k=60, cross-encoder top-N, recency/support boosts, user-turn weight, age decay `0.5 + 0.5/(1+age_days)` (hindsight, deja-vu).
11. Warn when files referenced by a session changed afterwards (deja-vu) — AIDrive can do this exactly via versions.
12. Background consolidation with **dry-run that only produces proposals**; deletion only as a confirmed proposal (MIRIX, tigerless, Letta) — never silent.
13. Identity rules: server-bound identity cannot be overridden by tool arguments; deny overrides grants; recalled content is evidence, not authority (LongMemory, powercontext) — matches AIDrive, make explicit.
14. Markdown export of approved knowledge as a portable, human-editable view (EverOS, tigerless) — also the basis for Passport.
15. Separate sensitive tier (MIRIX vault) → restricted visibility.

## Revised AIDrive memory plan (proposal, founder to approve order)

- **M1 Knowledge items + review inbox (~1.5 days):** typed items (decision, fact, constraint, preference, gotcha, attempt failed/partial/worked, keystone); evidence = exact file version or session event plus quote; support count; agents propose, owner approves/edits/rejects with reason in one inbox (handoffs included); `expected_version`; approval writes a revision; relations updates/extends/derives.
- **M2 Freshness, validity, conflicts (~0.5 day):** stale when a cited file changes; valid_from/invalid_at; contradiction on the same topic → disputed until resolved; strict (current) vs historical recall.
- **M3 Brief v2 (~0.5 day):** keystones first, then approved decisions, stale/disputed warnings, failed attempts for touched files (`precheck_file`), handoff, activity, sessions; session-file-changed warnings.
- **M4 Visibility (~0.5 day):** private/team/restricted per item and per captured session; private by default for captured sessions.
- **M5 Evaluation (~1 day):** 60 dev cases from the real project, recall@10 by language, random-k control, permission and staleness tests; decide on vectors + RRF from data.
- **M6 Automatic capture (~0.5 day):** Claude Code Stop hook, periodic Codex capture, per-project consent and retention.
- **Later:** handoff acknowledgement + task outcome; consolidation that only proposes; Markdown/Passport export; vectors with RRF and boosts if M5 requires.
