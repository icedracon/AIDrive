# 10-minute onboarding and matched continuation task

Same words for all six participants (PRODUCT.md mandatory gate 6: same task and instructions). The facilitator reads the script, then stays silent except for the listed prompts. Install ([INSTALL.md](INSTALL.md)) happens before this call and is timed separately.

## Before the call (facilitator)

- Participant has the API, web app and both assistants connected to **their own** project folder (`get_workspace` works in both).
- Ask permission to record screen and notes. No recording without a yes.
- Prepare the observation sheet (below). Start a timer when the script starts.

## Script (read aloud)

**0:00 — Frame (1 min).** "AIDrive keeps one shared state for your project that both Claude Code and Codex can read: what was decided, what was tried, what's next. Assistants can propose; only you approve. Today we try one handover on your real project. I'm testing the tool, not you. Think aloud."

**0:01 — First brief (2 min).** "In Claude Code, start a new session and say: *Get the AIDrive project brief and tell me what you know.*" Expect: empty or near-empty brief. Note whether they understand the empty state.

**0:03 — Real work with Claude (3 min).** "Pick a small real next step on this project, something you'd do today anyway. Ask Claude to do it. When it's done, say: *Record an AIDrive handoff: what you did, decisions, next step.*"

**0:06 — Approve (1 min).** "Open localhost:3000. Find what Claude proposed. Approve, edit or reject it — your choice." Note: time to find it, whether they read the evidence.

**0:07 — Continue with Codex (3 min).** "In Codex, new chat, say only: *Continue this project from AIDrive.* Don't explain anything else." Expect: Codex names Claude's decision, changed file and next step, then proposes or starts it.

**0:10 — Debrief (outside the 10 minutes, max 5 min).** Ask, don't pitch:
1. What did Codex know that you didn't have to tell it?
2. What did it get wrong or miss?
3. How do you normally do this handover today? How long does it take?
4. Would you use this tomorrow on this project? What would stop you?

Then: "Over the next week use it whenever you'd normally switch assistants. I won't remind you. On day 7 I'll ask you to run one command that prints counts only."

## Allowed facilitator prompts

Only these, and each one is logged as an assist: "Where would you look for that?", "What does the assistant say?", "Try the web app." Anything else (explaining a tool, fixing config) is a **coaching intervention** and the run does not count toward gate 6's "without live coaching".

## Observation sheet (one row per participant)

| Field | Value |
|---|---|
| Participant code (P1-P6), date, OS | |
| Install time (min), install trips hit (INSTALL.md trip #) | |
| Script start → Codex names Claude's decision (min:sec) | |
| Codex correct on: decision / changed file / next step (y/n each) | |
| Critical miss (contradicted an approved decision or a constraint) | |
| Manual corrections the participant had to type | |
| Assists / coaching interventions (count, what) | |
| Approve step: found without help (y/n), read evidence (y/n) | |
| Debrief answers 1-4 (verbatim, short) | |

## Matched continuation task (for the paired comparison)

The 10-minute run uses the participant's own project. The **matched** comparison needs the same task in two conditions — their current handover (handoff.md/CLAUDE.md/AGENTS.md or copy-paste) vs. AIDrive — counterbalanced: P1, P3, P5 do current-first; P2, P4, P6 AIDrive-first.

Task template (fill from the participant's project before the call; two different but equivalent steps, A and B, one per condition):

1. Assistant 1 (Claude Code) does step A/B and is told one **constraint** the participant states out loud (e.g. "don't change the public API", "keep it under N dependencies").
2. Handover: current method or AIDrive handoff + approval.
3. Assistant 2 (Codex) gets only "continue" (AIDrive) or the participant's usual handover text (current).
4. Stop when Codex takes a correct next action, or after 10 minutes.

Measure per condition: time to correct next action; constraint kept (y/n — breaking it is a critical error); manual corrections; approval time counts toward AIDrive's time (PRODUCT.md). Record both model versions shown by each client. Six participants give directional evidence only.
