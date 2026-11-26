-- Pilot metrics: counts only, no names, file names or text. Read-only.
-- The participant runs it on their own machine and pastes the output:
--   psql -h /var/run/postgresql -X -d aidrive_dev -f scripts/pilot-metrics.sql
-- (as a PostgreSQL superuser; `sudo -u postgres` cannot read a mode-700 home)
-- (Read-only transactions cannot create temp views, so the event CTE repeats.)
-- Day 0 is the calendar day of the space's first assistant key (connect).
\set ON_ERROR_STOP 1
BEGIN TRANSACTION READ ONLY;

\echo '== per space and day (days 0-14) =='
WITH ws AS (
    SELECT w.id, left(md5(w.id::text), 8) AS space,
           (SELECT min(g.created_at) FROM public.agent_grants g WHERE g.workspace_id = w.id) AS onboarded_at
    FROM public.workspaces w
), ev AS (
    SELECT workspace_id, created_at AS at, 'assistant_write' AS kind FROM public.file_actions WHERE agent_id IS NOT NULL
    UNION ALL SELECT workspace_id, created_at, 'assistant_handoff' FROM public.handoffs WHERE agent_id IS NOT NULL
    UNION ALL SELECT workspace_id, created_at, 'assistant_proposal' FROM public.knowledge_items WHERE agent_id IS NOT NULL
    UNION ALL SELECT workspace_id, decided_at, 'owner_decision' FROM public.handoffs WHERE decided_at IS NOT NULL AND NOT auto_accepted
    UNION ALL SELECT workspace_id, decided_at, 'owner_decision' FROM public.knowledge_items
        WHERE decided_at IS NOT NULL AND agent_id IS NOT NULL AND NOT auto_accepted
    UNION ALL SELECT workspace_id, captured_at, 'session_capture' FROM public.sessions
), pilot_events AS (
SELECT ws.space, ws.onboarded_at::date AS day0, ev.kind,
       (ev.at::date - ws.onboarded_at::date) AS day
FROM ev JOIN ws ON ws.id = ev.workspace_id
WHERE ws.onboarded_at IS NOT NULL
)
SELECT space, day,
       count(*) FILTER (WHERE kind LIKE 'assistant_%') AS assistant_actions,
       count(*) FILTER (WHERE kind = 'owner_decision') AS owner_decisions,
       count(*) FILTER (WHERE kind = 'session_capture') AS captures
FROM pilot_events WHERE day BETWEEN 0 AND 14
GROUP BY space, day ORDER BY space, day;

\echo '== summary: returned = an assistant action on day 1..7 =='
WITH ws AS (
    SELECT w.id, left(md5(w.id::text), 8) AS space,
           (SELECT min(g.created_at) FROM public.agent_grants g WHERE g.workspace_id = w.id) AS onboarded_at
    FROM public.workspaces w
), ev AS (
    SELECT workspace_id, created_at AS at, 'assistant_write' AS kind FROM public.file_actions WHERE agent_id IS NOT NULL
    UNION ALL SELECT workspace_id, created_at, 'assistant_handoff' FROM public.handoffs WHERE agent_id IS NOT NULL
    UNION ALL SELECT workspace_id, created_at, 'assistant_proposal' FROM public.knowledge_items WHERE agent_id IS NOT NULL
    UNION ALL SELECT workspace_id, decided_at, 'owner_decision' FROM public.handoffs WHERE decided_at IS NOT NULL AND NOT auto_accepted
    UNION ALL SELECT workspace_id, decided_at, 'owner_decision' FROM public.knowledge_items
        WHERE decided_at IS NOT NULL AND agent_id IS NOT NULL AND NOT auto_accepted
    UNION ALL SELECT workspace_id, captured_at, 'session_capture' FROM public.sessions
), pilot_events AS (
SELECT ws.space, ws.onboarded_at::date AS day0, ev.kind,
       (ev.at::date - ws.onboarded_at::date) AS day
FROM ev JOIN ws ON ws.id = ev.workspace_id
WHERE ws.onboarded_at IS NOT NULL
)
SELECT space, min(day0) AS day0,
       bool_or(kind LIKE 'assistant_%' AND day BETWEEN 1 AND 7) AS returned_d1_7,
       count(DISTINCT day) FILTER (WHERE kind LIKE 'assistant_%' AND day BETWEEN 0 AND 7) AS active_days_0_7,
       round(count(*) FILTER (WHERE kind = 'owner_decision' AND day BETWEEN 0 AND 7) / 8.0, 2) AS decisions_per_day_0_7
FROM pilot_events GROUP BY space ORDER BY day0;

\echo '== approval by exception (migration 0017): days 0-7 per space =='
-- auto_accepted: accepted without the owner; kept: the owner confirmed it;
-- undone: the owner reversed it. A high undo rate means auto-acceptance
-- approves the wrong things.
WITH ws AS (
    SELECT w.id, left(md5(w.id::text), 8) AS space,
           (SELECT min(g.created_at) FROM public.agent_grants g WHERE g.workspace_id = w.id) AS onboarded_at
    FROM public.workspaces w
), auto AS (
    SELECT workspace_id, created_at AS at, confirmed_at IS NOT NULL AS kept, undone_at IS NOT NULL AS undone
    FROM public.handoffs WHERE auto_accepted
    UNION ALL
    SELECT workspace_id, created_at, confirmed_at IS NOT NULL, status = 'retired' AND confirmed_at IS NULL
    FROM public.knowledge_items WHERE auto_accepted
)
SELECT ws.space,
       count(*) AS auto_accepted,
       count(*) FILTER (WHERE kept) AS kept,
       count(*) FILTER (WHERE undone) AS undone,
       round(count(*) FILTER (WHERE undone)::numeric / nullif(count(*), 0), 2) AS undo_rate
FROM auto JOIN ws ON ws.id = auto.workspace_id
WHERE ws.onboarded_at IS NOT NULL AND (auto.at::date - ws.onboarded_at::date) BETWEEN 0 AND 7
GROUP BY ws.space ORDER BY ws.space;

ROLLBACK;
