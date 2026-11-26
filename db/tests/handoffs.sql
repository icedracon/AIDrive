-- Run as aidrive_app on a database migrated through 0008 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('h-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('h-editor@example.test', :'hash') AS editor_id \gset
SELECT app.register_user('h-viewer@example.test', :'hash') AS viewer_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Handoffs') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'editor_id', 'editor'), (:'ws', :'viewer_id', 'viewer');
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'create', NULL, NULL, 'brief.md', 'v1', NULL) AS v1 \gset
SELECT app.issue_agent_grant(:'ws', '30000000-0000-4000-8000-0000000000c1', 'Claude', decode(repeat('c1', 32), 'hex'), 'files:write');
SELECT file_id FROM file_versions WHERE id = :'v1' \gset
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.file', :'file_id', false), set_config('test.v1', :'v1', false) \gset

-- Viewer cannot record.
BEGIN; SELECT set_config('app.user_id', :'viewer_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.record_handoff(current_setting('test.ws')::uuid, gen_random_uuid(), 's', '{}', 'n', '[]');
    RAISE EXCEPTION 'FAIL H01: viewer recorded a handoff';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS H01 viewer cannot record handoffs';
END $$;
ROLLBACK;

-- Agent records with an exact source version; attribution and idempotency.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('c1', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.record_handoff(:'ws', '40000000-0000-4000-8000-000000000001', 'Drafted the brief',
    ARRAY['Audience: solo founders'], 'Codex writes interview questions',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS h1 \gset
SELECT app.record_handoff(:'ws', '40000000-0000-4000-8000-000000000001', 'Drafted the brief',
    ARRAY['Audience: solo founders'], 'Codex writes interview questions',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS h1_replay \gset
SELECT set_config('test.h1', :'h1', false), set_config('test.h1r', :'h1_replay', false) \gset
DO $$ BEGIN
    IF current_setting('test.h1') <> current_setting('test.h1r') THEN RAISE EXCEPTION 'FAIL H02: replay created a duplicate'; END IF;
    IF (SELECT agent_id FROM handoffs WHERE id = current_setting('test.h1')::uuid) <> '30000000-0000-4000-8000-0000000000c1'
       OR (SELECT status FROM handoffs WHERE id = current_setting('test.h1')::uuid) <> 'proposed' THEN
        RAISE EXCEPTION 'FAIL H02: attribution or status';
    END IF;
    RAISE NOTICE 'PASS H02 agent handoff attributed, proposed, idempotent on replay';
    PERFORM app.record_handoff(current_setting('test.ws')::uuid, '40000000-0000-4000-8000-000000000001', 'Different', '{}', 'n', '[]');
    RAISE EXCEPTION 'FAIL H03: request reused with different input';
EXCEPTION WHEN SQLSTATE 'AD409' THEN RAISE NOTICE 'PASS H03 reused request ID with different input rejected';
END $$;
-- Agents cannot approve, even holding the owner's write key.
DO $$ BEGIN
    PERFORM app.decide_handoff(current_setting('test.ws')::uuid, current_setting('test.h1')::uuid, 'approved', NULL);
    RAISE EXCEPTION 'FAIL H04: agent approved';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS H04 agent cannot approve its own handoff';
END $$;
COMMIT;

-- Sources must belong to this workspace.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Elsewhere') AS other_ws \gset
SELECT set_config('app.workspace_id', :'other_ws', true) \gset
SELECT app.write_text_file(:'other_ws', gen_random_uuid(), 'create', NULL, NULL, 'x.md', 'x', NULL) AS foreign_v \gset
SELECT file_id AS foreign_file FROM file_versions WHERE id = :'foreign_v' \gset
SELECT set_config('app.workspace_id', :'ws', true), set_config('test.ff', :'foreign_file', true), set_config('test.fv', :'foreign_v', true) \gset
DO $$ BEGIN
    PERFORM app.record_handoff(current_setting('test.ws')::uuid, gen_random_uuid(), 's', '{}', 'n',
        jsonb_build_array(jsonb_build_object('file_id', current_setting('test.ff'), 'version_id', current_setting('test.fv'))));
    RAISE EXCEPTION 'FAIL H05: foreign source accepted';
EXCEPTION WHEN foreign_key_violation THEN RAISE NOTICE 'PASS H05 cross-workspace source rejected';
END $$;
ROLLBACK;

-- Editor cannot approve; owner can.
BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.decide_handoff(current_setting('test.ws')::uuid, current_setting('test.h1')::uuid, 'approved', NULL);
    RAISE EXCEPTION 'FAIL H06: editor approved';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS H06 editor cannot approve';
END $$;
ROLLBACK;
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.decide_handoff(:'ws', :'h1', 'approved', 'Looks right');
DO $$ BEGIN
    IF (SELECT status FROM handoffs WHERE id = current_setting('test.h1')::uuid) <> 'approved' THEN RAISE EXCEPTION 'FAIL H07'; END IF;
    PERFORM app.decide_handoff(current_setting('test.ws')::uuid, current_setting('test.h1')::uuid, 'rejected', NULL);
    RAISE EXCEPTION 'FAIL H07: decided twice';
EXCEPTION WHEN SQLSTATE 'AD409' THEN RAISE NOTICE 'PASS H07 owner approves once; second decision refused';
END $$;
COMMIT;

-- Content is immutable to the application role.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    UPDATE handoffs SET summary = 'rewritten';
    RAISE EXCEPTION 'FAIL H08: handoff rewritten';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS H08 handoff content cannot be rewritten';
END $$;
ROLLBACK;

-- A proposal whose source changed cannot be approved; the approved one becomes stale.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.record_handoff(:'ws', gen_random_uuid(), 'Second pass', '{}', 'Review',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS h2 \gset
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'update', :'file_id', :'v1', NULL, 'v2', NULL) AS v2 \gset
SELECT set_config('test.h2', :'h2', true) \gset
DO $$ BEGIN
    PERFORM app.decide_handoff(current_setting('test.ws')::uuid, current_setting('test.h2')::uuid, 'approved', NULL);
    RAISE EXCEPTION 'FAIL H09: approved with changed source';
EXCEPTION WHEN SQLSTATE 'AD412' THEN RAISE NOTICE 'PASS H09 approval refused after the cited source changed';
END $$;
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM handoff_sources s JOIN files f ON f.workspace_id = s.workspace_id AND f.id = s.file_id
                   WHERE s.handoff_id = current_setting('test.h1')::uuid AND f.current_version_id <> s.version_id) THEN
        RAISE EXCEPTION 'FAIL H10: staleness not observable';
    END IF;
    RAISE NOTICE 'PASS H10 approved handoff is detectably stale once its source changes';
END $$;
SELECT app.record_handoff(:'ws', gen_random_uuid(), 'Third pass', '{}', 'Ship',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v2'))) AS h3 \gset
SELECT app.decide_handoff(:'ws', :'h3', 'approved', NULL);
SELECT set_config('test.h3', :'h3', true) \gset
DO $$ BEGIN
    IF (SELECT status FROM handoffs WHERE id = current_setting('test.h1')::uuid) <> 'superseded'
       OR (SELECT count(*) FROM handoffs WHERE workspace_id = current_setting('test.ws')::uuid AND status = 'approved') <> 1 THEN
        RAISE EXCEPTION 'FAIL H11: supersession';
    END IF;
    RAISE NOTICE 'PASS H11 newer approval supersedes the older; one approved handoff remains';
END $$;
COMMIT;

\echo 'HANDOFF CHECKS PASSED'
