-- Run as aidrive_app on a database migrated through 0009 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('act-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('act-viewer@example.test', :'hash') AS viewer_id \gset
SELECT app.register_user('act-outsider@example.test', :'hash') AS outsider_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Activity') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'viewer_id', 'viewer');
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'create', NULL, NULL, 'plan.md', 'v1', NULL) AS v1 \gset
SELECT file_id FROM file_versions WHERE id = :'v1' \gset
SELECT app.issue_agent_grant(:'ws', '50000000-0000-4000-8000-0000000000d1', 'Codex', decode(repeat('d1', 32), 'hex'), 'files:write');
COMMIT;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('d1', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'update', :'file_id', :'v1', NULL, 'v2', NULL) AS v2 \gset
SELECT app.record_handoff(:'ws', gen_random_uuid(), 'Updated the plan', '{}', 'Review it',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v2'))) AS h \gset
COMMIT;
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.decide_handoff(:'ws', :'h', 'approved', NULL);
SELECT app.revoke_agent_grant(:'ws', '50000000-0000-4000-8000-0000000000d1');
COMMIT;
SELECT set_config('test.ws', :'ws', false) \gset

BEGIN; SELECT set_config('app.user_id', :'viewer_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ DECLARE kinds text[]; BEGIN
    SELECT array_agg(kind ORDER BY at DESC, kind) INTO kinds FROM app.workspace_activity(current_setting('test.ws')::uuid, NULL, 50);
    IF kinds IS DISTINCT FROM ARRAY['agent.revoked', 'handoff.approved', 'handoff.proposed', 'file.update', 'agent.issued', 'file.create'] THEN
        RAISE EXCEPTION 'FAIL V01: unexpected timeline %', kinds;
    END IF;
    RAISE NOTICE 'PASS V01 viewer sees the full timeline newest first';
END $$;
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM app.workspace_activity(current_setting('test.ws')::uuid, NULL, 50)
                   WHERE kind = 'file.update' AND agent_name = 'Codex' AND actor_email = 'act-owner@example.test' AND revision = 2)
       OR NOT EXISTS (SELECT 1 FROM app.workspace_activity(current_setting('test.ws')::uuid, NULL, 50)
                   WHERE kind = 'file.create' AND agent_id IS NULL AND subject = 'plan.md') THEN
        RAISE EXCEPTION 'FAIL V02: attribution';
    END IF;
    RAISE NOTICE 'PASS V02 agent changes name the agent and the accountable owner; human changes carry no agent';
END $$;
DO $$ BEGIN
    IF (SELECT count(*) FROM app.workspace_activity(current_setting('test.ws')::uuid, NULL, 2)) <> 2
       OR (SELECT count(*) FROM app.workspace_activity(current_setting('test.ws')::uuid,
            (SELECT min(at) FROM app.workspace_activity(current_setting('test.ws')::uuid, NULL, 2)), 50)) <> 4 THEN
        RAISE EXCEPTION 'FAIL V03: paging';
    END IF;
    RAISE NOTICE 'PASS V03 limit and before-cursor page through the timeline';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'outsider_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.workspace_activity(current_setting('test.ws')::uuid, NULL, 50);
    RAISE EXCEPTION 'FAIL V04: outsider read activity';
EXCEPTION WHEN no_data_found THEN RAISE NOTICE 'PASS V04 non-member gets not-found';
END $$;
ROLLBACK;

DO $$ BEGIN
    PERFORM app.workspace_activity(current_setting('test.ws')::uuid, NULL, 50);
    RAISE EXCEPTION 'FAIL V05: no-context read';
EXCEPTION WHEN no_data_found THEN RAISE NOTICE 'PASS V05 no context gets not-found';
END $$;

\echo 'ACTIVITY CHECKS PASSED'
