-- Run as aidrive_app on a database migrated through 0011 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('s-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('s-viewer@example.test', :'hash') AS viewer_id \gset
SELECT app.register_user('s-outsider@example.test', :'hash') AS outsider_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Sessions') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'viewer_id', 'viewer');
SELECT app.issue_agent_grant(:'ws', '70000000-0000-4000-8000-0000000000a1', 'Codex', decode(repeat('b1', 32), 'hex'), 'files:write');
SELECT app.issue_agent_grant(:'ws', '70000000-0000-4000-8000-0000000000a2', 'Reader', decode(repeat('b2', 32), 'hex'), 'files:read');
COMMIT;
SELECT set_config('test.ws', :'ws', false) \gset

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('b1', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.upsert_session(:'ws', 'codex', 'rollout-1', 'aidrive', 'import', '2026-11-19T10:00:00Z', '2026-11-19T11:00:00Z') AS sid \gset
SELECT app.upsert_session(:'ws', 'codex', 'rollout-1', 'aidrive', 'import', '2026-11-19T09:30:00Z', '2026-11-19T10:30:00Z') AS sid_again \gset
SELECT app.add_session_events(:'ws', :'sid', '[
  {"key":"e1","ordinal":0,"role":"user","at":"2026-11-19T10:00:00Z","text":"Should the pilot target solo founders?"},
  {"key":"e2","ordinal":1,"role":"assistant","at":"2026-11-19T10:01:00Z","text":"Yes: solo founders switching between Claude and Codex lose context most."},
  {"key":"e3","ordinal":2,"role":"tool","at":"2026-11-19T10:02:00Z","text":"tool: update_file handoff.md"}
]'::jsonb, false) AS first_insert \gset
SELECT app.add_session_events(:'ws', :'sid', '[
  {"key":"e2","ordinal":1,"role":"assistant","at":"2026-11-19T10:01:00Z","text":"Yes: solo founders switching between Claude and Codex lose context most."},
  {"key":"e4","ordinal":3,"role":"user","at":"2026-11-19T10:05:00Z","text":"Աուդիտորիան հայերեն էլ պետք է աշխատի"}
]'::jsonb, true) AS second_insert \gset
COMMIT;
SELECT set_config('test.sid', :'sid', false), set_config('test.sid2', :'sid_again', false),
       set_config('test.n1', :'first_insert', false), set_config('test.n2', :'second_insert', false) \gset
-- Sessions are private by default since 0015; the owner shares this one so
-- the viewer checks below exercise team visibility (db/tests/visibility.sql
-- covers private sessions).
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.set_session_visibility(:'ws', :'sid', 'team');
COMMIT;

BEGIN; SELECT set_config('app.user_id', :'viewer_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ DECLARE s record; BEGIN
    SELECT * INTO s FROM sessions WHERE id = current_setting('test.sid')::uuid;
    IF current_setting('test.sid') <> current_setting('test.sid2') OR current_setting('test.n1') <> '3' OR current_setting('test.n2') <> '1'
       OR s.event_count <> 4 OR s.coverage <> 'complete' OR s.started_at <> '2026-11-19T09:30:00Z' OR s.ended_at <> '2026-11-19T11:00:00Z'
       OR s.agent_id <> '70000000-0000-4000-8000-0000000000a1' THEN
        RAISE EXCEPTION 'FAIL S01: upsert/idempotency %', s;
    END IF;
    RAISE NOTICE 'PASS S01 re-capture merges bounds, repeated events are ignored, coverage and agent recorded';
END $$;
DO $$ BEGIN
    IF (SELECT count(*) FROM session_events WHERE search @@ websearch_to_tsquery('simple', 'solo founders')) <> 2
       OR (SELECT count(*) FROM session_events WHERE search @@ websearch_to_tsquery('simple', 'հայերեն')) <> 1
       OR (SELECT count(*) FROM session_events WHERE search @@ to_tsquery('simple', 'Աուդիտոր:*')) <> 1 THEN
        RAISE EXCEPTION 'FAIL S02: full-text search';
    END IF;
    RAISE NOTICE 'PASS S02 viewer finds events by English and Armenian words';
END $$;
DO $$ BEGIN
    PERFORM app.upsert_session(current_setting('test.ws')::uuid, 'codex', 'x', 'x', 'import', NULL, NULL);
    RAISE EXCEPTION 'FAIL S03: viewer captured';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS S03 viewer cannot capture sessions';
END $$;
DO $$ BEGIN
    PERFORM app.delete_session(current_setting('test.ws')::uuid, current_setting('test.sid')::uuid);
    RAISE EXCEPTION 'FAIL S04: viewer deleted';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS S04 viewer cannot delete sessions';
END $$;
ROLLBACK;

BEGIN;
DO $$ BEGIN
    PERFORM app.begin_agent_request(current_setting('test.ws')::uuid, decode(repeat('b2', 32), 'hex'), 'files:write');
    RAISE EXCEPTION 'FAIL S05: read key opened a capture context';
EXCEPTION WHEN SQLSTATE 'AD403' THEN RAISE NOTICE 'PASS S05 files:read key cannot capture';
END $$;
ROLLBACK;
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('b2', 32), 'hex'), 'files:read') IS NOT NULL AS ok \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM session_events) <> 4 THEN RAISE EXCEPTION 'FAIL S06: reader cannot read'; END IF;
    PERFORM app.delete_session(current_setting('test.ws')::uuid, current_setting('test.sid')::uuid);
    RAISE EXCEPTION 'FAIL S06: agent deleted';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS S06 files:read agent reads sessions; agents cannot delete';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'outsider_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM sessions) OR EXISTS (SELECT 1 FROM session_events) THEN RAISE EXCEPTION 'FAIL S07: outsider read'; END IF;
    RAISE NOTICE 'PASS S07 outsider sees no sessions';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    UPDATE session_events SET body = 'rewritten';
    RAISE EXCEPTION 'FAIL S08: event rewritten';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS S08 captured events cannot be rewritten';
END $$;
SELECT app.delete_session(:'ws', :'sid') AS deleted \gset
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM sessions) OR EXISTS (SELECT 1 FROM session_events)
       OR EXISTS (SELECT 1 FROM session_events WHERE search @@ websearch_to_tsquery('simple', 'founders')) THEN
        RAISE EXCEPTION 'FAIL S09: deletion left data';
    END IF;
    RAISE NOTICE 'PASS S09 owner deletion removes the session, its events and search entries';
END $$;
COMMIT;

\echo 'SESSION CHECKS PASSED'
