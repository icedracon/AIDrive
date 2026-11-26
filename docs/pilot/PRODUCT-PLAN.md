# AIDrive product plan for the five-person pilot

Decision brief, 2026-11-25. Proposed sequencing. [Product contract](../PRODUCT.md) and [ROADMAP](../../ROADMAP.md) govern release decisions.

## The job people hire it for

A developer finishes a change with Claude, opens Codex for a review, and gets a useful first action without reconstructing the conversation. The owner can see the source revision, correct a mistaken assumption, and tell which assistant acted on that correction. This is the pilot promise. The longer-term product extends that workflow to authorized agents that can request work, return results, and recover interrupted tasks under the same owner's supervision.

## Evidence and boundaries

| Surface | What we know | What the participant still needs demonstrated |
|---|---|---|
| Windows candidate 515f00e | An isolated profile install started the web app and watcher. The packaged MCP rejected a stale write with isError and preserved the later edit. | Cold install on a clean Windows machine with actual Claude Code and Codex sessions. Verify web sign-in, both workspace identities, and watcher after logon. |
| Chat routing | Real Claude and Codex chats exchanged the same Armenian challenge word. Claude reported automatic receipt through its Stop hook. Exact-chat addressing advanced separately on the chat-push branch. | Inventory the actual candidate. Show the owner a request arriving after the target becomes idle, its subsequent turn, and its reply. The new Desktop test stopped on tool approval. |
| Context | The AI campaign observed a sourced handoff and English retrieval. An Armenian memory query missed its English fact. | A blind continuation on a participant's real project, including a changed source and an unknown answer. |

## Experience to design

Use the existing Next.js dashboard. The first screen after install should answer “Which project am I in, and can both assistants see it?” Show folder, workspace, connected client names and the last successful check. Offer one repair action at the failed step. Show the actual local URL. Update the onboarding script's fixed port and approval instructions to match the candidate's configured URL and review mode.

Make the handoff the primary task. Show the next action, the source file at the cited revision, and what changed since. Put current and cited versions side by side for a stale source. For messages, offer a target picker with client, project folder, short chat ID and last activity. Two identically named chats must require an explicit choice.

In a task detail view, show “Stored”, “Delivered to chat”, “Turn started”, and “Reply received” only when the corresponding event exists. A pending item should say what will unblock it, for example “Waiting for the next Codex turn”. The owner can inspect first, then send a correction to the selected session. Confirm the destination and show its receipt. Keep transcripts private by default. Make status text readable by keyboard and screen reader, including without color.

## Engineering and algorithm choices

| Problem | Initial implementation | When to add complexity |
|---|---|---|
| Identity and routing | Derive sender from authenticated credentials. Resolve recipient by workspace and immutable chat ID. Keep native thread ID, AIDrive chat ID and process lifetime distinct. Test same-name and restarted clients. | Add roles and aliases to the picker once their resolution is deterministic. |
| Reliable activation | Persist request and dispatch intent in one PostgreSQL transaction. Lease outbox rows, retry with capped backoff, deduplicate by request ID. Record the native turn ID before acknowledging execution where possible. A crash after dispatch with no receipt enters reconciliation. | Add a persistent event stream if measured polling latency or cost warrants it. Never retry an ambiguous side effect blindly. |
| Turn coordination | One running turn per managed session, queued owner input, finite deadline and turn budget. Return an explicit unsupported outcome when the native client cannot wake or interrupt. | Add delegation graphs when a third agent has a demonstrated job. |
| Context selection | Apply workspace permissions, required rules and version checks before ranking. Compare lexical search, multilingual embeddings and reciprocal rank fusion on the same held-out EN/HY cases. Reserve budget for required evidence and allow abstention. | Ship the simplest method meeting the product's recall, abstention and latency targets. Measure token savings with correctness held constant. |
| Owner visibility | Rust API exposes an ordered event history with stable IDs. Next.js renders it and resumes from the last ID after reconnect. Scope reads and actions using existing PostgreSQL authorization. | Add live steering per adapter only after confirming the native outcome. |

## Work order and acceptance

1. **Engineering and QA — freeze a candidate.** Record branch tips, migration ledger, archive hash, supported client versions and review mode. Reconcile changes across installer, chat routing and OpenCode branches. Run migrations against a previous database and a blank one. Store per-check evidence against the archive people will receive.
2. **Design and frontend — test one handoff.** Implement the connection check, evidence view and honest delivery states in existing panels. Observe a newcomer install, switch assistants and resolve a stale source. Record where coaching was needed. Keep the map available as a secondary view.
3. **Backend and adapters — prove managed wake.** Register a new native thread, let its initial turn finish, then send a unique challenge from Claude. Start a turn on the exact registered target through the supported host interface. Capture timestamps, native turn ID, reply and one owner intervention. Show all events in the dashboard. A facilitator click, another prompt, or a still-running agent loop invalidates the idle-wake result.
4. **QA — rerun on the candidate.** Cover cross-workspace isolation, duplicate names, expired credentials, stale writes, owner correction during execution, receiver offline, restart between commit and dispatch, restart after dispatch, malicious source text and unavailable provider. For each case record expected state, observed state, recovery and remaining data. Use mocks for deterministic crash timing and real clients for activation and comprehension.
5. **Pilot and marketing — recruit for the demonstrated workflow.** Start with people who already switch between Claude Code and Codex weekly. Show a short recording of a real source-backed handoff. Ask the first participant to bring one current project. After a successful observed onboarding, invite the remaining four under the same protocol. Record installation failures and assistance alongside successes.

## Decision after five participants

Run the existing [matched task](ONBOARDING.md), counterbalance task order, and report the individual paired observations alongside medians. Measure time from opening the second assistant to its first correct continuation, critical errors, and manual restatement. Use the [tracker](TRACKER.md) for independent return on days 1–7, verified by activity plus interview. The product contract proposes at least 30% lower median continuation time without more critical errors, three of five independent returns, and two explicit paid-pilot commitments. Keep proposed targets distinct from results. If the first handoff works but participants do not return, interview them about the next actual switching occasion before expanding orchestration.
