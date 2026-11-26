# Session memory: what existing projects do, and what AIDrive takes

Research date: 2026-11-20. Sources are each project's own README/docs (read via web fetch; some pages truncated or summarised). threadctx could not be read (HTTP 402). Numbers and behaviours below are as documented by each project, not independently verified; "unverified" marks claims that only third parties or design documents make.

## Projects read

| Project | Licence | Capture | Store | Retrieval | Lifecycle | Notable |
| --- | --- | --- | --- | --- | --- | --- |
| [Memorix](https://github.com/AVIDS2/memorix) | Apache-2.0 | Client hooks (Claude Code, Codex, Cursor); no transcript reading found | SQLite canonical; FTS5/Orama/LanceDB as rebuildable derived indexes | BM25 or hybrid; source-aware boosts; `maxTokens`; progressive disclosure (search → detail → timeline) | Typed observations; candidate → qualified → approved → superseded; ADD/UPDATE/DELETE/NOOP consolidation; `capturedHash` freshness | "Derived indexes must never decide a failed DB is an empty project" |
| [agentmemory](https://github.com/rohitg00/agentmemory) | Apache-2.0 | Claude Code/Codex/Cursor hooks POST events; `import-jsonl` from `~/.claude/projects`; explicit MCP tools for others | Observations + memories + per-session digest; graph edges from co-occurrence (no LLM) | BM25 + vector + graph fused (RRF k=60 unverified); `TOKEN_BUDGET` 2000, whole blocks packed in rank order; filter by layer before candidate limit | Event-id idempotency + offline spool; version chains (superseded hidden, kept); decay (formula unverified) | Warns Claude Code deletes JSONL after `cleanupPeriodDays` (default 30) |
| [Beacon](https://github.com/Asymptote-Labs/agent-beacon) | MIT | OTLP + hooks + poll, normalised to one event model in local JSONL | SQLite `memory.db`, project-scoped | MCP `search_memory`/`get_memory_context`; recall ≤5 approved memories (algorithm undocumented) | Manual distillation: evaluator scores a trace, agent drafts the lesson, **user approves**; "no silent writes"; supersede | Pattern redaction keeps the label (`token=[REDACTED]`), runs before truncation, 4 KB per string, metadata-only mode; admits unlabeled secrets are missed |
| [projectmem](https://github.com/riponcm/projectmem) | MIT | git hooks, file watcher, explicit CLI/MCP writes | Append-only `events.jsonl` + derived summaries | Path matching, plain search, `get_context(tokens)`; capped summary sections | Never deletes; `--supersedes`; decision flagged stale when its file "predates N commits" | `attempt` outcomes failed/partial/worked; `precheck_file` warns what already failed in a file |
| [agent-memory-engine](https://github.com/Ninadnj/agent-memory-engine) | MIT | Explicit writes; optional Claude Code hooks; markdown import | One JSON file per project | Feature-hashing embedding (lexical by default), relevance floor, `budget_tokens` 300, skip blocks that do not fit | `expected_revision` on edits; supersede/forget; exact-text dedup only; no automatic summariser without measured reason | Honest eval with a random-k control; small author-labelled set |
| [mem0](https://github.com/mem0ai/mem0) | Apache-2.0 | SDK calls; Claude Code plugin via hooks | SQL + vector DB + entity store | Semantic + keyword + entity + temporal signals | Moved to single-pass ADD-only extraction; explicit update/delete by ID; scoping by user/agent/app/run IDs | Reports LoCoMo/LongMemEval gains for its hosted platform, not OSS |

Locally on the founder's machine: 134 Claude Code transcripts under `~/.claude/projects` and 183 Codex session files under `~/.codex/sessions` (JSONL). These files are internal client formats, not public APIs.

## Techniques adopted (ideas only; no code copied)

All licences are permissive (Apache-2.0/MIT). AIDrive re-implements ideas in its own Rust/PostgreSQL code; no source is copied, so no NOTICE obligations arise.

1. **Two capture paths.** Client hooks for live sessions plus an importer for existing transcripts (agentmemory, Memorix). Import early: Claude Code prunes old JSONL.
2. **Idempotent events.** Every event carries a stable ID (client event/tool-use ID or content hash); the server ignores repeats; the client spools when offline (agentmemory).
3. **Layers kept apart.** Raw session evidence, proposed observations and approved knowledge are separate, and filtering by layer happens before ranking limits (agentmemory, Memorix). This matches PRODUCT.md's three layers.
4. **Typed observations.** decision, problem-solution, gotcha, trade-off, and attempts with outcome failed/partial/worked (Memorix, projectmem), so an assistant can be warned about an approach that already failed on a file.
5. **Human approval for shared knowledge.** Candidate → approved → superseded, no silent writes, lessons cite the event IDs they came from (Beacon, Memorix). AIDrive already enforces this for handoffs.
6. **Supersede, never overwrite.** Old versions leave search but stay inspectable (agentmemory, engine, projectmem). Revision-checked edits (engine) correspond to AIDrive's conditional writes.
7. **Freshness from exact sources.** Content hash / commit comparison marks dependent memory stale (Memorix, projectmem). AIDrive already does this with file versions; extend it to sessions that cite files.
8. **Lexical retrieval first.** Full-text search as the default, vectors optional and only after measurement (Memorix FTS5 default, engine's "needs a measured reason"). For AIDrive: PostgreSQL full-text search with the `simple` configuration so Armenian and English both work, ranked by relevance and recency.
9. **Progressive disclosure under a token budget.** Compact hits first, full detail by ID, then a timeline; pack whole result blocks in rank order and drop what does not fit, never truncate mid-block (Memorix, agentmemory, engine).
10. **Session-start brief.** Fixed order: approved handoff and its freshness → unapproved proposals → recent failed attempts on relevant files → recent session summaries (adapted from agentmemory's block order).
11. **Redaction before storage.** Pattern-based, keeping the label, before truncation, with a per-string cap and a metadata-only option; state openly that unlabeled secrets can slip through (Beacon). Strip `<private>` blocks (agentmemory).
12. **Rebuildable indexes.** Search indexes are derived from the canonical rows and never treated as authoritative (Memorix).
13. **Honest evaluation.** Include a random-k control and per-language/category results (engine); reuse LongMemEval dimensions already named in PRODUCT.md.

## What AIDrive adds that these do not

- Shared, permissioned state across people and assistants on a server, with per-agent grants, revocation and workspace isolation enforced in the database.
- Owner approval tied to exact file versions, with staleness computed on every read.
- Conflict-safe shared files and an attributed activity timeline.

## Proposed AIDrive design (not implemented)

- **Data:** `sessions` (client, external session ID, project folder fingerprint, agent grant, capture origin hook/import, coverage complete/partial, started/ended) and `session_events` (ordinal, role user/assistant/tool, redacted text ≤ 4 KB, stable event hash unique per session). Observations reuse handoffs or a sibling table with typed kinds and event references.
- **Capture:** `aidrive-mcp-server capture --client claude|codex --project <folder>` imports only sessions whose working directory is inside the folder, redacts, and uploads idempotently. Automatic capture via a Claude Code session-end hook (verify the payload fields before relying on them); Codex by watching new session files. Opt-in per project; nothing outside the chosen folder is read.
- **Retrieval (MCP):** `search_sessions(query, budget_tokens)` returns compact hits; `get_session_excerpt(session, around_event)` returns detail; `get_project_brief()` assembles the session-start brief. Agents need `files:read` or higher; revocation applies immediately.
- **Control:** owner can delete a session (events and index entries go with it); capture coverage is shown next to every imported session.
- **Not in the first slice:** embeddings, LLM consolidation, decay. Add only after the evaluation shows lexical recall is insufficient.

## Open questions

- Exact fields of the Claude Code session-end hook payload and Codex session file schema on the installed versions (inspect locally before writing parsers).
- Whether automatic capture should be on by default per paired project or always explicit.
- Retention period for imported transcripts.
