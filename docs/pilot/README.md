# Pilot kit — 6 design partners

Goal (adviser verdict 2026-11-21): six developers who use both Claude Code and Codex on one long project install AIDrive and use it for a week. **Kill or pivot if fewer than 4 of 6 return within 7 days** (the adviser's 3 of 5, kept at the same share). This tests return use; the 30% continuation-time and paid-intent gates in PRODUCT.md need the matched task in [ONBOARDING.md](ONBOARDING.md).

| Doc | For |
|---|---|
| [INSTALL.md](INSTALL.md) | Participant: shortest install path, trip list, uninstall |
| [ONBOARDING.md](ONBOARDING.md) | Facilitator: 10-minute script, observation sheet, matched task |
| [OUTREACH.md](OUTREACH.md) | Founder: draft invitation (not sent) |
| [RECRUITING.md](RECRUITING.md) | Founder: screener, where to find people, Russian invitation |
| [DAY7-INTERVIEW.md](DAY7-INTERVIEW.md) | Facilitator: 15-minute day-7 interview (EN + RU) |
| [TRACKER.md](TRACKER.md) | Founder: blockers before inviting, funnel, P1-P6, decision rule |
| [../../scripts/pilot-metrics.sql](../../scripts/pilot-metrics.sql) | Participant runs on day 7, pastes output |
| [../research/mcp-security-review.md](../research/mcp-security-review.md) | What an assistant can and cannot be steered into |

## Timeline per participant

Day −1 install (async, with INSTALL.md; log time and trips) → day 0 10-minute call → days 1-7 no reminders → day 7 metrics + 15-minute interview → day 14 optional second metrics run.

Before day 0, decide how to handle **key expiry on day 7** (INSTALL.md trip #1): either ship the fix first, or tell every participant the same sentence ("if tools say the key expired, tell the assistant: connect"). Do not fix it for some participants only.

## Reading the metrics

No telemetry exists and none is added for the pilot: everything is local. On day 7 the participant runs

```bash
psql -h /var/run/postgresql -X -d aidrive_dev -f scripts/pilot-metrics.sql   # as yourself, after INSTALL step 1
```

It runs in a read-only transaction and prints counts per space and day; spaces appear as the first 8 hex of `md5(id)`, never names, file names or text. Day 0 is the calendar day (database time zone, usually UTC) of the space's first assistant key.

| Column | Meaning |
|---|---|
| `assistant_actions` | Assistant writes, handoffs and knowledge proposals that day |
| `owner_decisions` | Owner approvals/rejections of assistant handoffs and proposals |
| `captures` | Sessions imported |
| `returned_d1_7` | **The kill metric.** At least one assistant action on days 1-7 |
| `active_days_0_7` | Distinct days with assistant actions |
| `decisions_per_day_0_7` | Owner decisions over 8 days ÷ 8 |

Count a participant as **returned** only if `returned_d1_7 = t` **and** the interview confirms it was a real task they started themselves (not a test, not prompted by us). Record each reminder or support contact; a return after our nudge is not independent. Ignore spaces created during the call for demos.

Decision on day 7: ≥4/6 returned → continue to matched comparisons and paid-pilot question. <4/6 → stop recruiting; write down the stated reasons (debrief Q4, interview) and revise the hypothesis before building more.

**Approval by exception (migration 0017, merged):** `pilot-metrics.sql` now prints auto-accepted, kept, undone and the **undo rate** per space for days 0-7, and `owner_decisions` excludes auto-accepted records. A high undo rate means approval-by-exception is approving the wrong things.

## Recruitment criteria

Uses both Claude Code and Codex weekly on the same ongoing project; switched between them in the last 7 days; can run a local PostgreSQL; agrees to a day-7 count run. Exclude friends who will be polite. Observe their current handover before showing AIDrive (PRODUCT.md pilot plan).
