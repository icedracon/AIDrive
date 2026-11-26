-- Run as aidrive_app on a database migrated through 0020 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('i-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('i-viewer@example.test', :'hash') AS viewer_id \gset
SELECT app.register_user('i-outsider@example.test', :'hash') AS outsider_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('File index') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'viewer_id', 'viewer');
SELECT app.issue_agent_grant(:'ws', '62000000-0000-4000-8000-0000000000a1', 'Claude', decode(repeat('6a', 32), 'hex'), 'files:write');
SELECT app.issue_agent_grant(:'ws', '62000000-0000-4000-8000-0000000000a2', 'Reader', decode(repeat('6b', 32), 'hex'), 'files:read');
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.outsider', :'outsider_id', false) \gset

BEGIN; SELECT set_config('app.user_id', :'viewer_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.register_artifact(current_setting('test.ws')::uuid, 'C:\tmp\x.py', 'pc', NULL, NULL, 'scratch', NULL);
    RAISE EXCEPTION 'FAIL I01: viewer registered';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS I01 viewers cannot register files';
END $$;
ROLLBACK;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('6b', 32), 'hex'), 'files:read') IS NOT NULL AS ok \gset
DO $$ BEGIN
    PERFORM app.register_artifact(current_setting('test.ws')::uuid, 'C:\tmp\x.py', 'pc', NULL, NULL, 'scratch', NULL);
    RAISE EXCEPTION 'FAIL I02: files:read key registered';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS I02 read-only keys cannot register files';
END $$;
ROLLBACK;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('6a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.register_artifact(:'ws', 'C:\work\scratch\probe.py', 'pc', 'C:\work', 'claude/x', 'probe for the parser bug',
                             clock_timestamp() - interval '1 hour') AS a1 \gset
SELECT app.register_artifact(:'ws', 'C:\work\scratch\probe.py', 'pc', 'C:\work', 'claude/x', 'probe, still needed',
                             clock_timestamp() - interval '1 hour') AS a1b \gset
SELECT app.register_artifact(:'ws', 'C:\work\notes.md', 'pc', NULL, NULL, 'notes', clock_timestamp() + interval '3 days') AS a2 \gset
SELECT app.register_artifact(:'ws', 'C:\work\keep.md', 'pc', NULL, NULL, 'kept until someone decides', NULL) AS a3 \gset
SELECT set_config('test.a1', :'a1', false), set_config('test.a1b', :'a1b', false),
       set_config('test.a2', :'a2', false), set_config('test.a3', :'a3', false) \gset
DO $$ DECLARE r record; BEGIN
    SELECT * INTO r FROM artifacts WHERE id = current_setting('test.a1')::uuid;
    IF current_setting('test.a1') <> current_setting('test.a1b') OR r.purpose <> 'probe, still needed'
       OR r.agent_id <> '62000000-0000-4000-8000-0000000000a1' OR r.branch <> 'claude/x' THEN
        RAISE EXCEPTION 'FAIL I03: register/re-register';
    END IF;
    IF (SELECT count(*) FROM app.workspace_artifacts(current_setting('test.ws')::uuid, NULL, true, NULL, 100)) <> 1
       OR (SELECT id FROM app.workspace_artifacts(current_setting('test.ws')::uuid, NULL, true, 'pc', 100)) <> current_setting('test.a1')::uuid
       OR (SELECT count(*) FROM app.workspace_artifacts(current_setting('test.ws')::uuid, NULL, true, 'other', 100)) <> 0 THEN
        RAISE EXCEPTION 'FAIL I04: due list';
    END IF;
    BEGIN
        PERFORM app.register_artifact(current_setting('test.ws')::uuid, 'C:\far.md', 'pc', NULL, NULL, 'x', clock_timestamp() + interval '2 years');
        RAISE EXCEPTION 'FAIL I05: keep_until too far accepted';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
        PERFORM app.register_artifact(current_setting('test.ws')::uuid, '  ', 'pc', NULL, NULL, 'x', NULL);
        RAISE EXCEPTION 'FAIL I05: blank path accepted';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    RAISE NOTICE 'PASS I03 an assistant registers files with worktree and branch; registering the same path again updates it';
    RAISE NOTICE 'PASS I04 only active files past keep_until are due, per machine';
    RAISE NOTICE 'PASS I05 keep_until over 400 days and blank paths are refused';
END $$;
COMMIT;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('6a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ DECLARE ws uuid := current_setting('test.ws')::uuid; BEGIN
    -- The client found the due file still present and notes.md gone; any active row may be checked.
    IF app.record_artifact_checks(ws, jsonb_build_array(
           jsonb_build_object('id', current_setting('test.a1'), 'exists', true),
           jsonb_build_object('id', current_setting('test.a2'), 'exists', false),
           jsonb_build_object('id', gen_random_uuid(), 'exists', false))) <> 2 THEN
        RAISE EXCEPTION 'FAIL I06: check count';
    END IF;
    IF (SELECT status FROM artifacts WHERE id = current_setting('test.a1')::uuid) <> 'active'
       OR (SELECT last_seen_at FROM artifacts WHERE id = current_setting('test.a1')::uuid) IS NULL
       OR (SELECT status FROM artifacts WHERE id = current_setting('test.a2')::uuid) <> 'missing'
       OR (SELECT closed_at FROM artifacts WHERE id = current_setting('test.a2')::uuid) IS NULL THEN
        RAISE EXCEPTION 'FAIL I06: check effects';
    END IF;
    RAISE NOTICE 'PASS I06 checks close missing files and keep present ones due until someone decides';

    -- Separate statements: a STABLE list in the same expression would see the
    -- snapshot from before the update.
    IF app.resolve_artifact(ws, current_setting('test.a1')::uuid, 'extend', clock_timestamp() + interval '3 days', 'still testing') <> 'active' THEN
        RAISE EXCEPTION 'FAIL I07: extend';
    END IF;
    IF (SELECT count(*) FROM app.workspace_artifacts(ws, NULL, true, NULL, 100)) <> 0 THEN
        RAISE EXCEPTION 'FAIL I07: extended file still due';
    END IF;
    IF app.resolve_artifact(ws, current_setting('test.a3')::uuid, 'keep', NULL, NULL) <> 'kept' THEN
        RAISE EXCEPTION 'FAIL I07: keep';
    END IF;
    IF app.resolve_artifact(ws, current_setting('test.a1')::uuid, 'removed', NULL, 'deleted after the fix') <> 'removed' THEN
        RAISE EXCEPTION 'FAIL I07: removed';
    END IF;
    BEGIN
        PERFORM app.resolve_artifact(ws, current_setting('test.a3')::uuid, 'extend', NULL, NULL);
        RAISE EXCEPTION 'FAIL I07: extend without date';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
        PERFORM app.resolve_artifact(ws, gen_random_uuid(), 'keep', NULL, NULL);
        RAISE EXCEPTION 'FAIL I07: unknown id';
    EXCEPTION WHEN no_data_found THEN NULL;
    END;
    IF (SELECT count(*) FROM app.workspace_artifacts(ws, 'open', false, NULL, 100)) <> 1
       OR (SELECT count(*) FROM app.workspace_artifacts(ws, 'all', false, NULL, 100)) <> 3 THEN
        RAISE EXCEPTION 'FAIL I07: status filters';
    END IF;
    RAISE NOTICE 'PASS I07 extend, keep and removed are recorded; extend needs a future date';

    IF (SELECT count(*) FROM app.workspace_activity(ws, NULL, 100) WHERE kind = 'artifact.registered') <> 3
       OR (SELECT count(*) FROM app.workspace_activity(ws, NULL, 100) WHERE kind IN ('artifact.removed', 'artifact.missing')) <> 2 THEN
        RAISE EXCEPTION 'FAIL I08: activity';
    END IF;
    RAISE NOTICE 'PASS I08 the activity timeline shows registered, removed and missing files';
END $$;
COMMIT;

BEGIN; SELECT set_config('app.user_id', :'outsider_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM artifacts) <> 0 THEN RAISE EXCEPTION 'FAIL I09: outsider sees rows'; END IF;
    BEGIN
        PERFORM * FROM app.workspace_artifacts(current_setting('test.ws')::uuid, NULL, false, NULL, 10);
        RAISE EXCEPTION 'FAIL I09: outsider listed';
    EXCEPTION WHEN no_data_found THEN NULL;
    END;
    RAISE NOTICE 'PASS I09 people outside the space see no registered files';
END $$;
ROLLBACK;
