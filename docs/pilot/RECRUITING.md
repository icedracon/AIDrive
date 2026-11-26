# Recruiting 6 design partners

The adviser scored reachability 2/5: nobody is named yet. This page is the plan for finding six people. Contacting anyone is the founder's own action; nothing here has been sent.

## Who qualifies (screener)

Ask these before sending the install guide. All five "must" answers are required.

| # | Question | Must be |
|---|---|---|
| 1 | Which AI coding assistants did you use in the last 7 days? | Claude Code **and** Codex (CLI or desktop) |
| 2 | Did you switch between them on the **same** project this week? | Yes, at least once |
| 3 | How do you hand over context when you switch? | Any concrete answer (CLAUDE.md, AGENTS.md, handoff.md, copy-paste, "I re-explain") |
| 4 | Can you run PostgreSQL locally (Linux, macOS, or Windows with WSL)? | Yes |
| 5 | Will you run one command on day 7 that prints counts only, and talk for 15 minutes? | Yes |
| 6 | How long is the project going to continue? | Nice to have: at least 2 more weeks |
| 7 | Have you paid for any tool or spent hours building something to fix this handover? | Nice to have: a concrete example (strong signal) |

Exclude: close friends and family (they will be polite), people who only use one assistant, people evaluating for an employer without a project of their own, anyone who needs us to set it up for them.

## Funnel to plan for

Expect roughly 25-30 contacts → 8-10 interested → 7-8 installed → 6 complete day 7. These are planning guesses, not data; record the real numbers in [TRACKER.md](TRACKER.md).

## Where to look (most promising first)

1. **People who already described the pain in public.** Search for posts where someone says they switch between Claude Code and Codex, lose context between sessions, or keep a handoff file (X/Twitter, Hacker News comments, r/ClaudeAI, r/ChatGPTCoding, r/codex). Reply to the post itself first; a cold DM without context converts worse.
2. **Users of free DIY sharing tools.** Authors and issue-reporters of small Claude-to-Codex sharing projects (memcp, handoff tools, cross-agent memory repos listed in ADVISER.md Phase 1). They already proved the pain by building or installing something.
3. **Developer communities the founder belongs to.** ITMO and Armenian developer chats, Russian-speaking AI-coding Telegram groups, the Claude and OpenAI developer Discords. Acquaintances, not friends.
4. **Founder's own contacts who switch assistants daily.** At most 1-2 of the 6, and they still go through the screener.

Do not run ads or mass mailing for this pilot: five well-matched people beat fifty curious ones.

## Invitation — Russian version (not sent)

English and Armenian versions are in [OUTREACH.md](OUTREACH.md).

> Привет, {name}! Ты писал(а), что переключаешься между Claude Code и Codex на {project}. Я делаю небольшой локальный инструмент как раз для этой передачи: оба ассистента читают одно общее состояние проекта (решения, что уже пробовали, следующий шаг), и ничего не считается «согласованным», пока ты сам(а) это не подтвердишь. Всё работает только на твоём компьютере.
>
> Ищу 6 человек, чтобы неделю попробовать на реальном проекте: ~30 минут на установку, 10-минутный созвон, дальше просто пользуйся или нет. На 7-й день запускаешь одну команду, которая печатает только числа (ни код, ни текст никуда не уходят), и 15 минут разговора. Честное «это бесполезно» — самый полезный ответ.
>
> Интересно? Пришлю инструкцию по установке.

## After a yes

Send the screener answers back in one message, then [INSTALL.md](INSTALL.md); propose two slots for the 10-minute call ([ONBOARDING.md](ONBOARDING.md)); add the person to [TRACKER.md](TRACKER.md) as P1-P6. No reminders during days 1-7.
