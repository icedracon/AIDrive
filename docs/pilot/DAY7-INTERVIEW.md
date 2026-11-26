# Day-7 interview (15 minutes)

Same questions for all six. Ask about what happened, not about what they would do: past behaviour predicts, opinions flatter. Do not pitch, do not defend, do not explain a feature during the interview. Ask permission before taking notes.

Before the call: the participant has run [../../scripts/pilot-metrics.sql](../../scripts/pilot-metrics.sql) and pasted the output. Have it open.

## Questions

**1. The last switch (4 min)**
- "Tell me about the last time you switched from one assistant to the other this week. What were you working on?"
- "Did AIDrive play a part? What happened, step by step?"
- If they did not use it: "What did you do instead? What made you not use it?" (Most valuable answer of the pilot; get the real reason.)

**2. What it got right and wrong (3 min)**
- "What did the second assistant know without you telling it?"
- "What did it get wrong, miss or contradict? Did you have to correct it?"
- "Did anything get accepted automatically that you would not have approved? Did you undo anything?" (Compare with the auto_accepted / undone counts.)

**3. Effort (2 min)**
- "How much time did approving or reviewing take this week, roughly?"
- "Was there a moment it got in your way?"

**4. Value (3 min)**
- "If AIDrive disappeared tomorrow, what would you do instead?" Note the answer as: *very disappointed / somewhat / not at all*, based on what they say, not a scale you read out.
- "Have you paid for, built, or spent hours on anything to fix this handover before? What did it cost?"

**5. Commitment (2 min)**
- "I'm continuing with a few people for another month on the next version. Do you want in?" Record yes/no **and whether they gave a date** for the next session. A date is the commitment signal; "sure, maybe" is not.
- "Who else do you know who switches between assistants like you?" A name is a strong signal.

**6. Close (1 min)**
- "Anything I should have asked and didn't?"

## Record (one block per participant in TRACKER.md)

| Field | Value |
|---|---|
| Used it for a real task they started (y/n), how many times | |
| Reason when not used (verbatim) | |
| Second assistant: correct knowledge / wrong or missing (short) | |
| Unwanted auto-accepts, undos (count, what) | |
| Review time per week (their estimate) | |
| Disappointed if gone: very / somewhat / not | |
| Past spend on the problem (money or hours) | |
| Continue next month: yes with date / yes without date / no | |
| Referral name given (y/n) | |

## Russian version of the core questions

1. «Расскажи про последний раз на этой неделе, когда ты переключался(ась) с одного ассистента на другой. Над чем работал(а)? Участвовал ли AIDrive — что происходило по шагам?» Если нет: «Что ты сделал(а) вместо этого? Что помешало им воспользоваться?»
2. «Что второй ассистент знал без твоих объяснений? Что он сделал неправильно или упустил? Что-то принялось автоматически, чего ты бы не одобрил(а)? Что-нибудь отменял(а)?»
3. «Сколько времени примерно ушло на проверку и подтверждения? Был ли момент, когда он мешал?»
4. «Если AIDrive завтра исчезнет, что ты будешь делать? Ты раньше платил(а) или тратил(а) часы на решение этой проблемы?»
5. «Я продолжаю ещё месяц с несколькими людьми на следующей версии. Хочешь участвовать? Когда созвонимся?» и «Кого ещё ты знаешь, кто так же переключается между ассистентами?»
