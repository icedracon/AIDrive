# AI memory landscape: top 20 GitHub projects

Status 2026-11-20: 19 of the top 20 analysed from project READMEs/docs (read via web fetch; some pages truncated or summarised; all benchmark numbers are self-reported unless stated). Six were analysed earlier in [session-memory.md](session-memory.md); 13 more below. Findings, synthesis and the revised plan: [ai-memory-findings.md](ai-memory-findings.md).

## Ranked list (stars as shown on GitHub topic pages ai-memory / llm-memory / long-term-memory, read 2026-11-20)

| # | Project | Stars | One line | Status |
| --- | --- | --- | --- | --- |
| 1 | [thedotmack/claude-mem](https://github.com/thedotmack/claude-mem) | 119k | Captures agent session activity, compresses it, injects context into future sessions (Claude Code, Codex, Gemini) | Analysed in findings |
| 2 | [mem0ai/mem0](https://github.com/mem0ai/mem0) | 69.3k | Memory layer for agents and apps | Analysed (session-memory.md) |
| 3 | [vectorize-io/hindsight](https://github.com/vectorize-io/hindsight) | 62.1k | Agent memory that learns from interactions | Analysed in findings |
| 4 | [topoteretes/cognee](https://github.com/topoteretes/cognee) | 33.4k | Knowledge graph + vector memory platform | Analysed in findings |
| 5 | [supermemoryai/supermemory](https://github.com/supermemoryai/supermemory) | 31.7k | Memory and context engine, runnable locally | Analysed in findings |
| 6 | [getzep/graphiti](https://github.com/getzep/graphiti) | ~31k (other source) | Temporal knowledge graph (Zep's engine) | Analysed in findings |
| 7 | [TencentCloud/TencentDB-Agent-Memory](https://github.com/TencentCloud/TencentDB-Agent-Memory) | 31.1k | Team-level memory hub (chat memory, skills, wiki, code graph) | Analysed in findings |
| 8 | [rohitg00/agentmemory](https://github.com/rohitg00/agentmemory) | 30.5k | Cross-agent persistent memory | Analysed |
| 9 | [letta-ai/letta](https://github.com/letta-ai/letta) | ~25k (other source) | Tiered memory; agents edit their own memory; sleep-time compute | Analysed in findings |
| 10 | [MemoriLabs/Memori](https://github.com/MemoriLabs/Memori) | 17.6k | LLM-agnostic agent memory infrastructure | Analysed in findings |
| 11 | [EverMind-AI/EverOS](https://github.com/EverMind-AI/EverOS) | 14k | Portable, local-first, Markdown-native memory | Analysed in findings |
| 12 | [MemTensor/MemOS](https://github.com/MemTensor/MemOS) | 12.5k | Memory OS (MemCube, scheduling) | Analysed in findings |
| 13 | [plastic-labs/honcho](https://github.com/plastic-labs/honcho) | 8.2k | Peer model, derived representations | Analysed in findings |
| 14 | [CaviraOSS/LongMemory](https://github.com/CaviraOSS/LongMemory) | 4.6k | Local persistent memory for Claude Desktop/Copilot/Codex | Analysed in findings |
| 15 | [tigerless-labs/agent-memory](https://github.com/tigerless-labs/agent-memory) | 4.3k | Markdown memory; Claude Code and Codex share one store | Analysed in findings |
| 16 | [Asymptote-Labs/agent-beacon](https://github.com/Asymptote-Labs/agent-beacon) | 3.6k | Cross-harness capture, approved lessons | Analysed |
| 17 | [Mirix-AI/MIRIX](https://github.com/Mirix-AI/MIRIX) | 3.5k | Six memory types, multi-agent routing | Analysed in findings |
| 18 | [zilliztech/memsearch](https://github.com/zilliztech/memsearch) | 2.9k | Coding-agent memory on Markdown + Milvus | Analysed in findings |
| 19 | [MemTensor/memmy-agent](https://github.com/MemTensor/memmy-agent) | 2.2k | Local memory hub shared across Claude Code, Codex and others | Analysed in findings |
| 20 | [oceanbase/powercontext](https://github.com/oceanbase/powercontext) | 1.8k | Long-term memory and handoff for Claude Code and Codex | Analysed in findings (handoff) |
| + | [caura-ai/caura](https://github.com/caura-ai/caura) | 570 | "Governed shared memory for AI agent fleets" — closest positioning to AIDrive | Analysed in findings |
| + | [vshulcz/deja-vu](https://github.com/vshulcz/deja-vu) | 1.6k | Memory over existing coding-agent session files | Analysed in findings |

Also analysed earlier (below the top 20 by stars): [Memorix](https://github.com/AVIDS2/memorix) (943), [projectmem](https://github.com/riponcm/projectmem) (875), [agent-memory-engine](https://github.com/Ninadnj/agent-memory-engine).

Not on the topic pages (stars not confirmed there): Letta, Graphiti/Zep, LangMem, A-MEM, MemoryOS, Second-Me, basic-memory. Star counts from secondary sources (Sep 2026): mem0 ~66k, Graphiti ~31k, cognee ~31k, Letta ~25k ([atlan.com](https://atlan.com/know/best-ai-agent-memory-frameworks-2026/), [mnemoverse](https://mnemoverse.com/docs/library/ai-memory-solutions-2026-q3)).

## Question list for each project

1. Capture (hooks, transcript parsing, SDK, MCP; which clients).
2. Data model (turns, observations, facts, entities, graph, summaries, blocks, skills).
3. Retrieval (BM25/vector/hybrid/graph, reranking, temporal, token budget, injection point).
4. Lifecycle (extraction, dedup, consolidation, decay, contradictions, temporal validity/versioning, staleness).
5. Governance (approval, permissions, multi-user/team, audit, deletion, redaction).
6. Evaluation (benchmarks, numbers, self-reported or not).
7. Licence, stars, last update.
Specific: claude-mem compression and session-start injection; hindsight "learns from interactions"; Graphiti fact invalidation (valid_at/invalid_at); Letta self-editing memory and sleep-time compute; TencentDB team hub permissions; honcho peers; MemOS MemCube/scheduling; MIRIX six types; powercontext handoff; caura governance model.

## Draft AIDrive memory plan (before the 20-project analysis; to be revised)

Current state (commit bd541fe): session capture with local redaction, Armenian/English prefix full-text search, budgeted recall, project brief, owner-approved handoffs with exact versions and staleness, activity timeline, pairing for Claude Code/Codex.

Proposed next phases (founder has not yet approved the order):
- M1 Knowledge items with approval (~1 day): typed items decision/fact/constraint/preference/gotcha/attempt(failed|partial|worked); evidence = exact file version or session event; agents propose, owner approves/corrects/rejects; corrections create revisions; approved handoff decisions become items.
- M2 Freshness and conflicts (~0.5 day): stale when cited file changes; same topic + different approved values → disputed until the owner resolves.
- M3 Brief v2 + precheck (~0.5 day): brief starts with approved decisions and stale/disputed warnings; `precheck_file` reports earlier failed attempts.
- M4 Evaluation (~1 day): 60 dev cases from the real project, recall@10 by language, random-k control, permission tests; decide on embeddings from data.
- M5 Automatic capture (~0.5 day): install Claude Code Stop hook, periodic Codex capture, per-project consent and retention in the web UI.
- Candidates to evaluate after the 20-project analysis: temporal fact invalidation (Graphiti), consolidation/sleep-time jobs (Letta), team-level hubs (TencentDB), governance model (caura).
