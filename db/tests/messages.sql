-- Run as aidrive_app on a database migrated through 0016 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('m-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('m-viewer@example.test', :'hash') AS viewer_id \gset
SELECT app.register_user('m-outsider@example.test', :'hash') AS outsider_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Messages') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'viewer_id', 'viewer');
SELECT app.issue_agent_grant(:'ws', '60000000-0000-4000-8000-0000000000a1', 'Claude', decode(repeat('5a', 32), 'hex'), 'files:write');
SELECT app.issue_agent_grant(:'ws', '60000000-0000-4000-8000-0000000000a2', 'Codex', decode(repeat('5b', 32), 'hex'), 'files:write');
SELECT app.issue_agent_grant(:'ws', '60000000-0000-4000-8000-0000000000a3', 'Reader', decode(repeat('5c', 32), 'hex'), 'files:read');
SELECT app.issue_agent_grant(:'ws', '60000000-0000-4000-8000-0000000000a4', 'Gone', decode(repeat('5d', 32), 'hex'), 'files:write');
SELECT app.revoke_agent_grant(:'ws', '60000000-0000-4000-8000-0000000000a4');
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'create', NULL, NULL, 'plan.md', 'Plan v1', NULL) AS v1 \gset
SELECT file_id FROM file_versions WHERE id = :'v1' \gset
SELECT app.create_workspace('Other M') AS other_ws \gset
SELECT set_config('app.workspace_id', :'other_ws', true) \gset
SELECT app.write_text_file(:'other_ws', gen_random_uuid(), 'create', NULL, NULL, 'x.md', 'x', NULL) AS ov \gset
SELECT file_id AS ofile FROM file_versions WHERE id = :'ov' \gset
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.file', :'file_id', false), set_config('test.v1', :'v1', false),
       set_config('test.ofile', :'ofile', false), set_config('test.ov', :'ov', false) \gset

BEGIN; SELECT set_config('app.user_id', :'viewer_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.send_message(current_setting('test.ws')::uuid, gen_random_uuid(), 'assistants', NULL, 'hi', NULL, '[]');
    RAISE EXCEPTION 'FAIL G01: viewer sent';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS G01 viewers cannot send messages';
END $$;
ROLLBACK;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('5a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.send_message(:'ws', '61000000-0000-4000-8000-000000000001', 'assistants', 'Review', 'Please review plan.md', NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS m1 \gset
SELECT app.send_message(:'ws', '61000000-0000-4000-8000-000000000001', 'assistants', 'Review', 'Please review plan.md', NULL, '[]') AS m1r \gset
SELECT set_config('test.m1', :'m1', false), set_config('test.m1r', :'m1r', false) \gset
DO $$ BEGIN
    IF current_setting('test.m1') <> current_setting('test.m1r')
       OR (SELECT agent_id FROM messages WHERE id = current_setting('test.m1')::uuid) <> '60000000-0000-4000-8000-0000000000a1'
       OR (SELECT count(*) FROM message_sources WHERE message_id = current_setting('test.m1')::uuid) <> 1 THEN
        RAISE EXCEPTION 'FAIL G02: send';
    END IF;
    BEGIN
        PERFORM app.send_message(current_setting('test.ws')::uuid, '61000000-0000-4000-8000-000000000001', 'assistants', NULL, 'changed', NULL, '[]');
        RAISE EXCEPTION 'FAIL G02: changed replay accepted';
    EXCEPTION WHEN SQLSTATE 'AD409' THEN NULL;
    END;
    BEGIN
        PERFORM app.send_message(current_setting('test.ws')::uuid, gen_random_uuid(), 'Gone', NULL, 'x', NULL, '[]');
        RAISE EXCEPTION 'FAIL G03: revoked recipient';
    EXCEPTION WHEN SQLSTATE 'AD421' THEN NULL;
    END;
    BEGIN
        PERFORM app.send_message(current_setting('test.ws')::uuid, gen_random_uuid(), 'assistants', NULL, 'x', NULL,
            jsonb_build_array(jsonb_build_object('file_id', current_setting('test.ofile'), 'version_id', current_setting('test.ov'))));
        RAISE EXCEPTION 'FAIL G04: foreign source';
    EXCEPTION WHEN foreign_key_violation THEN NULL;
    END;
    RAISE NOTICE 'PASS G02 assistant message attributed, cites a file version, idempotent; changed replay, revoked recipient and foreign source refused';
END $$;
COMMIT;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('5b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.send_message(:'ws', gen_random_uuid(), 'claude', NULL, 'Reviewed: looks good', :'m1', '[]') AS m2 \gset
SELECT set_config('test.m2', :'m2', false) \gset
DO $$ BEGIN
    IF (SELECT thread_id FROM messages WHERE id = current_setting('test.m2')::uuid) <> current_setting('test.m1')::uuid
       OR (SELECT count(*) FROM app.workspace_messages(current_setting('test.ws')::uuid, NULL, NULL, true, true, 50)) <> 1 THEN
        RAISE EXCEPTION 'FAIL G05: thread or Codex inbox';
    END IF;
    RAISE NOTICE 'PASS G05 replies join the thread; an assistant''s inbox holds broadcasts it did not send';
END $$;
COMMIT;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('5a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    IF (SELECT array_agg(id) FROM app.workspace_messages(current_setting('test.ws')::uuid, NULL, NULL, true, true, 50)) <> ARRAY[current_setting('test.m2')::uuid] THEN
        RAISE EXCEPTION 'FAIL G06: direct reply not in Claude inbox';
    END IF;
    PERFORM app.mark_messages_read(current_setting('test.ws')::uuid, ARRAY[current_setting('test.m2')::uuid]);
    IF EXISTS (SELECT 1 FROM app.workspace_messages(current_setting('test.ws')::uuid, NULL, NULL, true, true, 50)) THEN
        RAISE EXCEPTION 'FAIL G06: read message still unread';
    END IF;
    RAISE NOTICE 'PASS G06 a direct reply reaches its recipient, and marking it read is per reader';
END $$;
COMMIT;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('5c', 32), 'hex'), 'files:read') IS NOT NULL AS ok \gset
DO $$ BEGIN
    IF (SELECT array_agg(id) FROM app.workspace_messages(current_setting('test.ws')::uuid, NULL, NULL, true, true, 50)) <> ARRAY[current_setting('test.m1')::uuid]
       OR app.mark_messages_read(current_setting('test.ws')::uuid, ARRAY[current_setting('test.m1')::uuid]) <> 1 THEN
        RAISE EXCEPTION 'FAIL G07: reader inbox';
    END IF;
    PERFORM app.send_message(current_setting('test.ws')::uuid, gen_random_uuid(), 'assistants', NULL, 'x', NULL, '[]');
    RAISE EXCEPTION 'FAIL G07: read key sent';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS G07 a files:read key reads broadcasts (not others'' direct messages) and marks them read, but cannot send';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM app.workspace_messages(current_setting('test.ws')::uuid, NULL, NULL, false, false, 50)) <> 2
       OR (SELECT count(*) FROM app.workspace_messages(current_setting('test.ws')::uuid, NULL, NULL, true, false, 50)) <> 0
       OR NOT EXISTS (SELECT 1 FROM app.workspace_activity(current_setting('test.ws')::uuid, NULL, 50)
                      WHERE kind = 'message.sent' AND agent_name = 'Codex') THEN
        RAISE EXCEPTION 'FAIL G08: owner view';
    END IF;
    INSERT INTO messages (workspace_id, thread_id, actor_id, recipient, body, request_id)
    VALUES (current_setting('test.ws')::uuid, gen_random_uuid(), current_setting('app.user_id')::uuid, 'owner', 'forged', gen_random_uuid());
    RAISE EXCEPTION 'FAIL G08: direct insert';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS G08 the owner sees every message and the activity entry; nothing is written around the functions';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'outsider_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM messages) THEN RAISE EXCEPTION 'FAIL G09: outsider read'; END IF;
    PERFORM app.workspace_messages(current_setting('test.ws')::uuid, NULL, NULL, false, false, 50);
    RAISE EXCEPTION 'FAIL G09: outsider listed';
EXCEPTION WHEN no_data_found THEN RAISE NOTICE 'PASS G09 outsiders see no messages';
END $$;
ROLLBACK;

\echo 'MESSAGE CHECKS PASSED'
