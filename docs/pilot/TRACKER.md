# Pilot tracker

Fill this in as the pilot runs. Participant codes only (P1-P6); keep names and contacts outside the repository.

## Before inviting anyone (blockers)

| # | Blocker | Why it matters | Status |
|---|---|---|---|
| 1 | Assistant keys expire after 7 days (`0005_agent_grants.sql`) | On day 7 every tool answers "invalid key", which breaks the day-7 return measurement itself (INSTALL.md trip #1) | Fixed 2026-11-22: keys live 30 days (migration 0018); the expired-key hint now says to run `connect` again |
| 2 | INSTALL.md does not include `install-hooks` (automatic capture and brief, MCP 0.0.15) | Without it a cut-off session loses its state, and the brief is not loaded automatically | Fixed 2026-11-22: INSTALL.md step 5b |
| 3 | New spaces default to review mode `exceptions` (migration 0017) | Participants see auto-accepted items; the undo rate becomes a metric | Decided: keep; measured by pilot-metrics.sql |
| 4 | Install is long on Windows (WSL PostgreSQL, Rust twice) | Most likely drop-off (INSTALL.md trip #2) | Open: one-command install is T2's next task |
| 5 | README intro states that storage and memory are not implemented | Contradicts the product a participant just installed | Fixed 2026-11-22 |

## Funnel

| Stage | Count | Notes |
|---|---|---|
| Contacted | 0 | |
| Interested (passed screener) | 0 | |
| Installed | 0 | |
| Day-0 call done | 0 | |
| Day 7 done | 0 | |

## Participants

| Code | Source | Screener ok | Install date, minutes, trips | Day-0 call | Day 7 date | Returned d1-7 (metrics + interview) | Assists / coaching | Disappointed if gone | Continue with date |
|---|---|---|---|---|---|---|---|---|---|
| P1 | | | | | | | | | |
| P2 | | | | | | | | | |
| P3 | | | | | | | | | |
| P4 | | | | | | | | | |
| P5 | | | | | | | | | |
| P6 | | | | | | | | | |

Counterbalancing for the matched task (ONBOARDING.md): P1, P3, P5 current method first; P2, P4, P6 AIDrive first.

## Decision on day 7 (written before the pilot starts)

- **4 or more of 6 returned on their own** (metrics show an assistant action on days 1-7 and the interview confirms a real task they started without a nudge): continue to the matched comparisons and the paid-pilot question.
- **Fewer than 4**: stop recruiting. Write down the stated reasons, then choose between the adviser's alternatives (team/agency audit layer; git-native handoff protocol plus CLI) or a revised hypothesis before building more.
- A high undo rate (say above 0.3) means approval by exception accepts the wrong things, whatever the return rate.
