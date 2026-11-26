# AIDrive product plan

Planning baseline: 2026-11-17. This document specifies intended behavior, not implemented functionality.

Release decision: the 9/10 acceptance contract below and the execution gates in ROADMAP.md govern the first complete pilot release. A polished prototype cannot satisfy them.

## Product promise

A shared workspace for people and their AIs: the same files, shared memory, and continuous work.

An authorized AI can discover previous conversations, inspect other AIs' recorded actions and outcomes, understand accepted decisions, and continue a project. The user owns the workspace and controls which content each participant can access.

"Shared understanding" means a source-backed, revisable project state. It does not mean model weights are shared, models always agree, or private model reasoning is available. We retain observable messages, actions, supplied explanations, evidence and results.

## Confirmed direction and proposed starting scope

The founder's confirmed direction is a common drive with shared conversation history, files, memories, and visibility into the work of other AIs. The long-term scope includes multiple people and agents working together.

Proposed first pilot: one owner per workspace, multiple isolated projects, and two AI provider adapters in the AIDrive chat interface. Initial pilot participants are people already using at least two AI tools on continuing projects. This is a recruitment hypothesis, not a permanent restriction to programming tasks.

Start with project briefs, Markdown notes, plain text and source files. Proposed initial bounds: 10 MB per file and 100 files per pilot workspace, configurable by the operator. PDF extraction, office editing, images, audio and video follow once the core loop is reliable. A file is searchable only after extraction/indexing completes, and the UI must show that status.

## First complete user journey

1. Create a workspace and see its sharing and retention settings.
2. Upload a brief and supporting files. Show persisted files and exact versions.
3. Start a conversation with AI A. Save visible messages in that workspace; AI can retrieve authorized sources.
4. AI A proposes a file edit and a project decision, with a concise rationale and source links.
5. Review the diff and approve the decision. Persist a new file version, approved memory and activity event.
6. Start a separate conversation with AI B in the same workspace. It receives the current project summary and retrieves relevant earlier messages and files.
7. AI B explains the previous work with citations, continues the task, and proposes its own changes.
8. Inspect both AIs' activity, correct a memory, compare file versions, or restore an earlier version.
9. Export a destination-specific Passport with selected decisions, tasks, source versions and next steps.

Changing the provider must not require the user to re-upload the same sources or manually re-explain already approved decisions. It does not guarantee identical model behavior.

## Six product surfaces

| Surface | User question | Primary actions |
| --- | --- | --- |
| Workspace overview | Where are we and what needs me? | Read project state, open pending reviews, resume a task |
| Drive | What are we working on? | Upload, browse, read, inspect proposed edits, compare and restore versions |
| Conversations | What have we discussed with each AI? | Start a chat, select a provider, search prior chats, follow citations |
| Shared memory | What does this project know? | Review candidates, approve, correct, reject, forget, inspect evidence |
| Activity | Who did what, and what happened? | Inspect actions, changed files, actual results, failures and handoffs |
| Connections and Passport | Who can access what? | Connect/revoke an agent, scope access, preview and export context |

Provider choice, capture status and review requirements must be visible. An empty workspace must show actual empty state, not fabricated files, usage or agent activity.

## Three layers of memory

### 1. Source history

Store captured conversations, file versions and observable action results with workspace, actor, source system, timestamps and provenance. History remains searchable until the user deletes it or its retention period expires. Capture gaps and failed imports must be visible.

### 2. Project knowledge

Maintain facts, preferences, decisions, constraints, task status, unanswered questions and handoff summaries. Each entry references its evidence and carries a revision and status: proposed, approved, disputed, stale, superseded or rejected. Keep observed action outcomes distinct from an agent's claim of success.

Automatic extraction creates candidates. Each space has a review mode (2026-11-21, after the adviser review; ROADMAP "T1 approval by exception"): in strict review the owner approves every lasting fact and decision; in review by exception (default for new spaces) a proposal with current evidence, no conflict, not a rule and not a correction is accepted at once, labelled as not checked by the owner and listed for one-click keep or undo, while every exception waits for the owner. Summaries may be generated automatically but must be labeled as derived and must not turn unsupported claims into accepted facts.

### 3. Context for the current task

Build a bounded context packet containing authorized project rules, applicable approved memories, current file versions, task state and relevant history. Record the selected source IDs and versions so the user can inspect what an AI received. Retrieve further authorized material when needed; do not put every conversation into every model request.

## Memory and collaboration rules

- All captured conversations in a shared project are discoverable by participants with the relevant permission. Private/global user memories are separate and never inherited silently.
- The first release stores conversations made inside AIDrive. External conversations require an explicitly supported capture integration or import. MCP tool access alone is not a promise of complete external transcript capture.
- Track capture coverage separately from the source history: complete for a captured turn, partial, failed or imported. No badge may claim complete provider-account history without evidence.
- Identify the human/agent connection, provider/model when known, task and request for each action. A recorded explanation is not private chain of thought.
- Show who proposed and who approved a decision. Two agents agreeing is not sufficient to make it true.
- When sources change, mark dependent memories for review. Deterministic source-version changes are sufficient for the first release; semantic contradiction detection is a later enhancement.
- When approved statements conflict, preserve both with disputed status and ask the owner to resolve them. Do not silently overwrite one with the newest response.
- An edit must declare its base file version. Reject an outdated write and offer a diff/retry. Start with sequential AI actions; autonomous concurrent execution comes later.
- Revocation immediately prevents future retrieval and actions. Previously delivered content cannot be recalled from an external provider.
- Deletion must remove source content and derived indexes/caches; dependent memories must be removed, redacted or explicitly invalidated. Provide a clear account of backup expiry and previously exported copies.
- Restoring a file creates a new version and triggers memory revalidation. It does not pretend to reverse external actions or erase history.

## First release scope

Must have: owner authentication, project isolation, durable files, versions and safe edits; two in-app provider adapters; persistent conversations; source-linked memory approval; retrieval across prior conversations and files; per-agent access; an activity timeline; context preview; selective Passport export and validated import into another authorized AIDrive workspace.

External MCP integration follows the same service authorization and version rules. A first external client must support context retrieval and explicit handoff creation; support for arbitrary clients is not assumed.

Deferred: autonomous agent teams, arbitrary shell execution, scheduled workflows, organization billing, enterprise SSO, third-party Drive sync, live collaborative office editing, whole-project rollback, and adoption of Passport as an industry standard. Preserve the long-term unified workspace vision while limiting the first implementation surface.

## Demonstration and acceptance gates

Use a real project with two providers, two separate conversations, two file revisions, an approved decision, a conflicting proposal, and a recorded task result.

| Gate | Required observable result |
| --- | --- |
| Persistence | Files, chats, approved memories and provenance survive service restart |
| Continuity | AI B identifies AI A's decision, rationale, changed file and next task without manual restatement |
| Evidence | Every accepted memory has resolvable source references; cited deleted/unavailable sources are clearly labeled |
| Freshness | A changed source invalidates the old derived memory before it is presented as current |
| Conflict handling | Contradictory proposals remain visible; no silent replacement of approved facts |
| Safe editing | A stale base version cannot overwrite a later file revision |
| Permissions | Denied content is excluded from search, summaries, context packets, exports and writes; revocation is enforced |
| Capture honesty | Unsupported external transcript capture is marked unavailable or partial |
| Recovery | Provider failures and repeated requests do not fabricate success, duplicate actions or lose existing files |
| User control | Correction, rejection, export and deletion have observable effects on subsequent retrieval |
| Portability | Export/import preserves selected decision identities, provenance, source versions and unsupported-field warnings |

These are release gates to implement and verify, not checks already passed.

## Pilot and learning plan

Recruit five design partners who have an ongoing project and actually switch AI tools. Observe their current workflow before demonstrating AIDrive. Ask about their most recent transfer, lost information, correction effort, frequency, privacy constraints and current workaround. Do not treat praise of the idea as demand.

Compare continuation on matched tasks using manual notes, a suitable existing solution and AIDrive. Keep model versions and task conditions recorded; vary task/order to reduce learning bias. Measure time to a correct next action, critical missed constraints, manual corrections, retrieved-source validity and per-task operating cost. Small pilot results are directional evidence.

Proposed go/no-go thresholds: at least 30% lower median continuation time with no increase in critical errors; three of five participants independently returning for a second real task within a week; two agreeing to a paid pilot with explicit terms. Record failures and revise the product hypothesis if these gates are missed. These numbers are proposed targets, not market benchmarks.

Initial business hypothesis: charge for a workspace with shared continuity and controlled usage. Determine the price after observing actual usage and willingness to pay. Track provider token costs, extraction, storage, retrieval, support and backups; avoid promising unlimited AI usage. No new subscriptions or paid infrastructure are approved by this plan.

## Implementation status and strategic context

Existing foundation: API health/readiness handlers, a web shell, a database migration with a restricted application role and a locally passing isolation suite (docs/DATABASE.md), an unverified Compose file, CI configuration, lockfiles, and a local MCP health tool. There is no functioning shared memory or document workflow yet. Compose/Docker validation, API-to-database connectivity and live service connectivity remain unresolved. CI has been written but not observed running remotely.

Hexa's public DriveX brief already includes a common filesystem, continuation across AI tools, version history, provenance and per-agent permissions. AIDrive's proposed explicit memory approval, structured Passport and freshness-aware continuation add specificity to that vision; they are not proven exclusive advantages. Source: https://www.hexa.com/ideas/drivex (read 2026-11-17).

Detailed conversations and the full 215-feature proposal have not been supplied. This plan covers the founder's direction established in this chat and the supplied technical roadmap.

## Interaction study — 2026-11-17

The [interactive prototype](../../outputs/AIDrive-interactive-prototype.html) covers the six product surfaces using a scripted project, two example AI participants and local in-memory state. Refresh resets the example. It makes no model calls and implements no persistence, authentication or actual provider permissions. The sample Passport is a local JSON download, not an established interoperable schema.

Design evidence checked on 2026-11-17:

| Source | Applied decision | Tradeoff |
| --- | --- | --- |
| [Microsoft Human–AI Interaction guidelines](https://www.microsoft.com/en-us/research/project/guidelines-for-human-ai-interaction/) | Explicit capability labels, approval/rejection and visible retry states | Manual approval adds interaction cost; test whether users understand its purpose |
| [Google PAIR: Explainability & Trust](https://pair.withgoogle.com/guidebook-v2/chapter/explainability-trust/) | Show exact source versions and the context delivered to the next AI | More evidence can add clutter; keep it accessible alongside the decision |
| [Nielsen Norman Group: Progressive Disclosure](https://www.nngroup.com/articles/progressive-disclosure/) | Keep primary actions visible and secondary explanations expandable | Hidden details may be missed; observe evidence discovery during user testing |

These sources inform the interaction design; they do not establish demand or demonstrate that this particular interface is usable.

Demo task: inspect brief.md v1; approve Claude's proposed audience; continue with Codex; inspect the delivered context; change the brief to v2; observe stale-memory exclusion; open the original evidence; review the current source and approve the revised decision; revoke Codex access; inspect blocked continuation/export; restore access and download a selective sample Passport. Separate recovery cases cover rejection/reopening, provider timeout/retry, a stale-base write and restoring old content as a new version. The fixture audience is an example, not a founder decision.

Verification: `node work/check-prototype.cjs` from the parent workspace passed six generated views and the state transitions above, including historical-source resolution, export content and reset. This is a Node VM check with a stub document, not a browser or backend test. The embedded browser rejected the local file URL under its security policy; a later localhost check covered rendering, the approval/continuation path, 375 px layout and keyboard focus (ROADMAP AID-001); screen-reader and user validation remain unperformed.

Next observation: ask a prospective user to complete the core task without hints. Record whether they can explain which decision is approved, identify its exact source, distinguish the previous AI response from current knowledge, recover from a changed source and predict what is exported. Capture failures before changing the interface. User testing and the five-partner pilot remain pending.

## 9/10 acceptance contract — 2026-11-17

This is an internal, evidence-based release rubric for the defined pilot scope, not a claim of market leadership, statistical certainty or completion of the entire long-term vision. The earlier 2/10 implementation rating was qualitative; it is not a baseline measurement under this new rubric. No new score is claimed until evidence is collected.

Scope: one human owner for each project workspace, two real in-app model providers, durable text files and captured conversations, reviewable shared memory, recorded actions, bounded source-backed context, selective Passport export/import and one explicitly tested external MCP client. A person may own multiple isolated workspaces. For this release, a workspace is the project isolation boundary; do not introduce nested projects with ambiguous access inheritance. Multi-user product workflows and autonomous concurrent agent teams remain later work.

Planning provider default: Anthropic and OpenAI API adapters behind a common interface; actual model IDs, availability, terms, token limits and prices must be verified before integration. An API adapter is not the Claude Code or Codex application. Display the actual provider/model; call an integration Claude Code or Codex only after testing that client. No model subscription is assumed to include API access or spending permission.

### Scoring rules

Each semicolon-separated item below is worth exactly 5 points, awarded only when its linked acceptance checks pass on the release candidate. Missing, blocked, simulated or failed evidence earns zero. Total: 20 items, 100 possible points. Keep a dated evidence record in ROADMAP.md with commit, environment, command/task, result and artifact location. The candidate needs at least 90 points AND every mandatory gate below; points cannot compensate for a privacy, integrity or core-function failure. A later defect invalidates the affected evidence.

| Category | Maximum | Five-point items |
| --- | ---: | --- |
| Shared work and continuity | 20 | Durable files with version-safe edits; two real providers with persisted conversations; AI B continues AI A with correct decision/source/task; observable actions and results with trustworthy attribution |
| Memory quality and control | 20 | Proposals/approvals/revisions with exact evidence; correction/conflict/staleness workflow; retrieval meets held-out evidence targets; grounded answers and appropriate abstention meet targets |
| Authorization and data lifecycle | 20 | Authenticated workspace isolation; scoped agent grants and revocation; deletion propagated to derived data and restore handling; import/document content cannot grant authority or bypass approvals |
| Reliability and operation | 15 | Reproducible migrations/startup/upgrade; restart/crash/retry behavior passes; backup restoration and operational diagnostics demonstrated |
| Usability | 10 | All six screens complete real tasks with keyboard/error/loading states; observed first-use task completion meets pilot usability target |
| Portability | 10 | Validated selective Passport round trip; one external MCP client retrieves context and records an explicit handoff within granted scope |
| Demonstrated user value | 5 | Pilot meets the continuation-time, error, return-use and paid-intent thresholds defined below |

### Mandatory gates: no exceptions for a 9/10 claim

1. The complete first user journey works against real storage and both real providers, survives restart, and is reproduced on two independent projects with provider order reversed. No scripted reply counts.
2. No known unresolved critical/high issue affecting privacy, authorization, attribution, data loss or the core journey. All deterministic denial, stale-write, deletion and retry cases pass. Zero unauthorized disclosures or mutations observed in the adversarial suite; this is evidence within tested scope, not a guarantee of perfect security.
3. Source changes cannot leave old memory eligible as current; revocation blocks new delivery/actions; deleted content is not retrievable through indexes, summaries, caches or exports. Previously delivered copies are explicitly outside recall control.
4. Backup restoration is demonstrated in an isolated environment, including file bytes, database state and deletion tombstones. Supported deployment and upgrade paths are exercised, not just described.
5. Memory evaluation passes separately for each provider. No known critical constraint violation or forbidden-source exposure may be hidden by an average score.
6. At least four of five new pilot users complete the core journey without live coaching, with no critical error; all five receive the same task/instructions. At least three return independently for another real task within seven days. Recruitment, observation and consent remain pending.
7. At least 90/100 with no missing mandatory evidence. Broad public release and claims of product-market fit require additional evidence beyond this small pilot.

### Memory evaluation protocol

Build 60 development cases and a separate locked 100-case held-out set before tuning retrieval. Keep entire project histories separate between sets. Include English and Armenian queries, changed facts, dates, several conversations, actual prior action results, conflicting decisions, absent answers, revoked/deleted sources and adversarial document instructions. Report results by language and category; each must have at least 10 held-out cases, with overlapping tags allowed. Do not silently drop failed or timed-out cases.

Label expected source IDs/versions, mandatory constraints, forbidden sources, expected next step, and when abstention is required. Evaluate retrieval separately from answer generation. Proposed product targets, not external benchmarks:

- Retrieval: mean recall@10 of required eligible evidence >=95% across answerable cases, >=90% per language/category. Exclude abstention-only cases from this denominator and report them separately. A case's recall is retrieved required sources divided by all required sources; make cases fit within the ten-source budget.
- Answers: >=90% case pass rate per provider, >=80% per language/category. A pass requires the correct current decision/next step, all mandatory constraints, and no material unsupported claim. Cite exact available source versions; citation resolvability must be 100% and audited claim support >=95%.
- Abstention: >=95% of unanswerable cases must explicitly acknowledge insufficient evidence and avoid inventing a decision. Historical facts may be quoted as historical, never presented as current after invalidation.
- Permissions: zero forbidden-source inclusion in retrieval, context manifests or outputs in the deterministic denial suite. Test both obvious requests and indirect retrieval through summaries/history.

Run the held-out set three times per provider on the release candidate with frozen model IDs, prompts, retrieval configuration and token budgets. Each run must meet the thresholds; record latency, failures and costs. Human-review outputs against the rubric; an LLM judge may assist but cannot be the sole release authority. If a held-out failure is used for tuning, move that case to development and replenish the held-out set with an unseen project case before claiming a new result. Budget and credentials must be available before live evaluation; lack of access is recorded as blocked, never replaced by mock success.

The dimensions are informed by [LongMemEval](https://arxiv.org/abs/2410.10813), which evaluates extraction, multi-session and temporal reasoning, updates and abstention. Our permission/version/workflow tests are additional product-specific requirements; our score is not a LongMemEval result. Source checked 2026-11-17.

### Value, usability and operating bounds

Observe five participants completing at least two matched continuation tasks in their current workflow and in AIDrive; counterbalance task/order and include an existing alternative where feasible. Proposed value gate: >=30% lower median paired continuation time, no increase in critical errors, three of five independent returns within seven days, and two expressing willingness to pay for the same concrete pilot offer. Report raw paired results and failures; this small sample does not establish market fit. Contacting participants, spending or publishing still requires the existing applicable authorization.

The usability task includes finding an exact source, explaining the context AI B received, correcting stale memory, and predicting an export's contents. Approval time counts toward task time. If approval overhead erases the benefit, improve candidate quality and review grouping. Review by exception is not silent: auto-accepted records stay labelled and audited, and their undo rate is reported with the approval metric.

Proposed performance test envelope: 100 text files (<=10 MB each), 10,000 messages, 1,000 memory revisions per workspace, ten active workspaces and ten concurrent non-generation requests on a recorded reference machine. Target p95 metadata reads <=500 ms and context assembly <=2 s, with <1% unexpected application errors across 1,000 mixed requests after warm-up. Report cold-start results separately. Provider latency and upload/indexing are measured separately; a provider timeout must leave an explicit recoverable state. These are targets to validate, not current capabilities.

Before pilot activation, record per-user daily provider budgets, output-token ceilings, allowed models, retention and backup-expiry policy. Budget admission is enforced before a call; reserve a conservative upper-bound cost and reconcile recorded usage. Alert at 80%; reject new calls that exceed the configured limit. No unlimited-use promise. Proposed initial backup objectives for the pilot: recovery point <=24 hours and recovery time <=60 minutes, verified by an actual restore; process crashes must not lose already acknowledged committed work.

## Cross-agent memory landscape — 2026-11-20

Observed via web search (project pages and third-party reviews; licences, activity and security not yet checked): several open-source projects already offer shared memory across coding-agent sessions over MCP — [Memorix](https://github.com/AVIDS2/memorix) (many clients incl. Claude Code, Codex, Cursor), [agentmemory](https://www.agent-memory.dev/) (Apache-2.0 per its page; plugins for Claude Code, Codex CLI, Cursor), [Beacon](https://explainx.ai/blog/beacon-asymptote-labs-open-source-cross-harness-agent-memory-claude-code-codex-cursor-2026) (captures Claude Code/Codex/Cursor sessions and distils approved lessons), [projectmem](https://github.com/riponcm/projectmem) (local, MIT), [agent-memory-engine](https://github.com/Ninadnj/agent-memory-engine) and [threadctx](https://threadctx.dev/) (team layer is paid). Locally, Claude Code and Codex keep full session transcripts as JSONL files (134 and 183 on the founder's machine), which is how such tools can capture whole sessions.

Implication: automatic session recall alone is not a differentiator. AIDrive's distinct position is governed shared state — owner-approved decisions tied to exact source versions with freshness, conditional shared files, per-agent grants and revocation, and attributed activity. Founder decision 2026-11-20: study the existing projects, adopt their best techniques and build AIDrive's own session memory on top of its governed model. Comparison, adopted techniques and the proposed design: [research/session-memory.md](research/session-memory.md). Hypothesis, not yet tested with users.
