-- Run as aidrive_app on a database migrated through 0023 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('c-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('c-outsider@example.test', :'hash') AS outsider_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Chats') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
SELECT app.issue_agent_grant(:'ws', 'c2000000-0000-4000-8000-0000000000a1', 'Claude', decode(repeat('3a', 32), 'hex'), 'files:write');
SELECT app.issue_agent_grant(:'ws', 'c2000000-0000-4000-8000-0000000000a2', 'Codex', decode(repeat('3b', 32), 'hex'), 'files:write');
SELECT app.issue_agent_grant(:'ws', 'c2000000-0000-4000-8000-0000000000a3', 'Reader', decode(repeat('3c', 32), 'hex'), 'files:read');
SELECT app.create_workspace('Other C') AS other_ws \gset
SELECT set_config('app.workspace_id', :'other_ws', true) \gset
SELECT app.issue_agent_grant(:'other_ws', 'c2000000-0000-4000-8000-0000000000b1', 'Stranger', decode(repeat('3d', 32), 'hex'), 'files:write');
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.other_ws', :'other_ws', false) \gset

-- Chats: Claude A and B share one key; Codex X and Y share another.
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.upsert_chat(:'ws', 'claude', 'claude-session-a', 'C:\proj') AS chat_a \gset
SELECT app.upsert_chat(:'ws', 'claude', 'claude-session-b', 'C:\proj') AS chat_b \gset
SELECT app.upsert_chat(:'ws', 'claude', 'claude-session-a', NULL) AS chat_a2 \gset
COMMIT;
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.upsert_chat(:'ws', 'codex', 'codex-thread-x', 'C:\proj') AS chat_x \gset
SELECT app.upsert_chat(:'ws', 'codex', 'codex-thread-y', 'C:\proj') AS chat_y \gset
COMMIT;
BEGIN; SELECT app.begin_agent_request(:'other_ws', decode(repeat('3d', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.upsert_chat(:'other_ws', 'claude', 'stranger-session', NULL) AS chat_s \gset
COMMIT;
SELECT set_config('test.a', :'chat_a', false), set_config('test.b', :'chat_b', false), set_config('test.x', :'chat_x', false),
       set_config('test.y', :'chat_y', false), set_config('test.s', :'chat_s', false), set_config('test.a2', :'chat_a2', false) \gset

BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    IF current_setting('test.a') <> current_setting('test.a2')
       OR (SELECT cwd FROM chats WHERE id = current_setting('test.a')::uuid) <> 'C:\proj'
       OR (SELECT count(*) FROM app.workspace_chats(current_setting('test.ws')::uuid, 30)) <> 4 THEN
        RAISE EXCEPTION 'FAIL C01: upsert';
    END IF;
    BEGIN
        PERFORM app.upsert_chat(current_setting('test.ws')::uuid, 'claude', 'bad session id', NULL);
        RAISE EXCEPTION 'FAIL C01: bad session accepted';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
        PERFORM app.upsert_chat(current_setting('test.ws')::uuid, 'gpt', 's1', NULL);
        RAISE EXCEPTION 'FAIL C01: bad client accepted';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    RAISE NOTICE 'PASS C01 hooks register a chat once per client session under their key; bad ids refused';
END $$;
COMMIT;

BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    IF app.upsert_chat(current_setting('test.ws')::uuid, 'opencode', 'mcp-4242-1795000000', 'C:\proj') IS NULL
       OR app.upsert_chat(current_setting('test.ws')::uuid, 'other', 'mcp-4243-1795000000', NULL) IS NULL THEN
        RAISE EXCEPTION 'FAIL C01b: server-registered chat';
    END IF;
    RAISE NOTICE 'PASS C01b an MCP server registers its own process as a chat for clients without hooks (OpenCode, others)';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.upsert_chat(current_setting('test.ws')::uuid, 'claude', 'owner-session', NULL);
    RAISE EXCEPTION 'FAIL C02: a person registered a chat';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS C02 only assistant keys register chats';
END $$;
ROLLBACK;

-- Claude chat A asks Codex; exactly one Codex chat claims it.
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.send_message(:'ws', 'c3000000-0000-4000-8000-000000000001', 'assistants', 'Question', 'What is the price?', NULL, '[]', :'chat_a', NULL) AS m1 \gset
COMMIT;
SELECT set_config('test.m1', :'m1', false) \gset
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.claim_chat_messages(:'ws', :'chat_x', false) AS claim_x \gset
SELECT app.claim_chat_messages(:'ws', :'chat_y', false) AS claim_y \gset
SELECT set_config('test.claim_x', :'claim_x', false), set_config('test.claim_y', :'claim_y', false) \gset
DO $$ BEGIN
    IF jsonb_array_length(current_setting('test.claim_x')::jsonb->'messages') <> 1
       OR current_setting('test.claim_x')::jsonb#>>'{messages,0,id}' <> current_setting('test.m1')
       OR current_setting('test.claim_x')::jsonb#>>'{messages,0,from_chat}' <> current_setting('test.a')
       OR current_setting('test.claim_x')::jsonb#>>'{messages,0,from_client}' <> 'claude'
       OR jsonb_array_length(current_setting('test.claim_y')::jsonb->'messages') <> 0
       OR EXISTS (SELECT 1 FROM app.workspace_messages(current_setting('test.ws')::uuid, NULL, NULL, true, true, 50)) THEN
        RAISE EXCEPTION 'FAIL C03: claim once % %', current_setting('test.claim_x'), current_setting('test.claim_y');
    END IF;
    RAISE NOTICE 'PASS C03 a message for a key is claimed by exactly one chat of that key and is then read for the key';
END $$;
-- Codex X replies without naming a chat: it goes back to Claude chat A.
SELECT app.send_message(:'ws', gen_random_uuid(), 'claude', NULL, 'About 40 dollars', :'m1', '[]', :'chat_x', NULL) AS m2 \gset
SELECT set_config('test.m2', :'m2', false) \gset
COMMIT;

BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.claim_chat_messages(:'ws', :'chat_b', true) AS claim_b \gset
SELECT app.claim_chat_messages(:'ws', :'chat_a', true) AS claim_a \gset
SELECT set_config('test.claim_a', :'claim_a', false), set_config('test.claim_b', :'claim_b', false) \gset
DO $$ BEGIN
    IF (SELECT to_chat FROM messages WHERE id = current_setting('test.m2')::uuid) <> current_setting('test.a')::uuid
       OR jsonb_array_length(current_setting('test.claim_b')::jsonb->'messages') <> 0
       OR current_setting('test.claim_a')::jsonb#>>'{messages,0,id}' <> current_setting('test.m2')
       OR (SELECT chat_id FROM message_deliveries WHERE message_id = current_setting('test.m2')::uuid) <> current_setting('test.a')::uuid
       OR NOT (SELECT wake FROM message_deliveries WHERE message_id = current_setting('test.m2')::uuid) THEN
        RAISE EXCEPTION 'FAIL C04: reply targeting % %', current_setting('test.claim_a'), current_setting('test.claim_b');
    END IF;
    RAISE NOTICE 'PASS C04 a reply goes to the chat its parent came from; another chat of the same key does not get it';
END $$;
DO $$ BEGIN
    IF NOT (app.claim_chat_messages(current_setting('test.ws')::uuid, current_setting('test.a')::uuid, true)->>'rate_limited')::boolean THEN
        RAISE EXCEPTION 'FAIL C05: second wake within 30 s allowed';
    END IF;
    RAISE NOTICE 'PASS C05 a chat is woken at most once every 30 seconds';
END $$;
COMMIT;

BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    BEGIN
        PERFORM app.send_message(current_setting('test.ws')::uuid, gen_random_uuid(), 'codex', NULL, 'x', NULL, '[]', NULL, current_setting('test.b')::uuid);
        RAISE EXCEPTION 'FAIL C06: to_chat of another key';
    EXCEPTION WHEN SQLSTATE 'AD422' THEN NULL;
    END;
    BEGIN
        PERFORM app.send_message(current_setting('test.ws')::uuid, gen_random_uuid(), 'codex', NULL, 'x', NULL, '[]', current_setting('test.x')::uuid, NULL);
        RAISE EXCEPTION 'FAIL C06: from_chat of another key';
    EXCEPTION WHEN SQLSTATE 'AD422' THEN NULL;
    END;
    BEGIN
        PERFORM app.send_message(current_setting('test.ws')::uuid, gen_random_uuid(), 'claude', NULL, 'x', NULL, '[]', NULL, current_setting('test.s')::uuid);
        RAISE EXCEPTION 'FAIL C06: chat of another space';
    EXCEPTION WHEN SQLSTATE 'AD422' THEN NULL;
    END;
    BEGIN
        PERFORM app.claim_chat_messages(current_setting('test.ws')::uuid, current_setting('test.x')::uuid, false);
        RAISE EXCEPTION 'FAIL C06: claimed another key''s chat';
    EXCEPTION WHEN SQLSTATE 'AD422' THEN NULL;
    END;
    BEGIN
        INSERT INTO message_deliveries (workspace_id, message_id, agent_id, chat_id, wake)
        VALUES (current_setting('test.ws')::uuid, current_setting('test.m1')::uuid, 'c2000000-0000-4000-8000-0000000000a1', current_setting('test.a')::uuid, false);
        RAISE EXCEPTION 'FAIL C06: direct delivery insert';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    RAISE NOTICE 'PASS C06 chats are checked against keys and spaces; claims and deliveries only through the functions';
END $$;
-- A chat can write to another chat of its own key.
SELECT app.send_message(:'ws', gen_random_uuid(), 'claude', NULL, 'Note from A to B', NULL, '[]', :'chat_a', :'chat_b') AS m3 \gset
SELECT set_config('test.m3', :'m3', false) \gset
SELECT app.claim_chat_messages(:'ws', :'chat_b', false) AS claim_b2 \gset
SELECT set_config('test.claim_b2', :'claim_b2', false) \gset
DO $$ BEGIN
    IF current_setting('test.claim_b2')::jsonb#>>'{messages,0,id}' <> current_setting('test.m3')
       OR jsonb_array_length(app.claim_chat_messages(current_setting('test.ws')::uuid, current_setting('test.a')::uuid, false)->'messages') <> 0 THEN
        RAISE EXCEPTION 'FAIL C07: same-key chat message %', current_setting('test.claim_b2');
    END IF;
    RAISE NOTICE 'PASS C07 a chat can write to another chat of its own key; the sending chat does not get it back';
END $$;
COMMIT;

BEGIN; SELECT set_config('app.user_id', :'outsider_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM chats) OR EXISTS (SELECT 1 FROM message_deliveries) THEN RAISE EXCEPTION 'FAIL C08: outsider read'; END IF;
    PERFORM app.workspace_chats(current_setting('test.ws')::uuid, 30);
    RAISE EXCEPTION 'FAIL C08: outsider listed';
EXCEPTION WHEN no_data_found THEN RAISE NOTICE 'PASS C08 outsiders see no chats or deliveries';
END $$;
ROLLBACK;

-- Loop guard: seven automatic wakes in one thread; the seventh is held and
-- the owner is told once; an owner message in the thread lets it through.
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.send_message(:'ws', gen_random_uuid(), 'codex', 'Loop', 'start', NULL, '[]', :'chat_a', :'chat_x') AS loop0 \gset
SELECT count(app.upsert_chat(:'ws', 'claude', 'loop-' || n, NULL)) AS loop_chats FROM generate_series(1, 7) n \gset
COMMIT;
SELECT set_config('test.loop0', :'loop0', false) \gset
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$
DECLARE v_chat uuid; v_res jsonb; v_ok int := 0;
BEGIN
    FOR n IN 1..7 LOOP
        SELECT id INTO v_chat FROM chats WHERE session_id = 'loop-' || n;
        PERFORM app.send_message(current_setting('test.ws')::uuid, gen_random_uuid(), 'claude', NULL, 'ping ' || n,
            current_setting('test.loop0')::uuid, '[]', NULL, v_chat);
    END LOOP;
    PERFORM set_config('test.ok', '0', false);
END $$;
COMMIT;
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$
DECLARE v_res jsonb; v_ok int := 0; v_paused int := 0;
BEGIN
    FOR n IN 1..7 LOOP
        v_res := app.claim_chat_messages(current_setting('test.ws')::uuid, (SELECT id FROM chats WHERE session_id = 'loop-' || n), true);
        v_ok := v_ok + jsonb_array_length(v_res->'messages');
        v_paused := v_paused + jsonb_array_length(v_res->'paused_threads');
    END LOOP;
    v_res := app.claim_chat_messages(current_setting('test.ws')::uuid, (SELECT id FROM chats WHERE session_id = 'loop-7'), true);
    IF v_ok <> 6 OR v_paused <> 1 OR jsonb_array_length(v_res->'paused_threads') <> 1
       OR (SELECT count(*) FROM messages WHERE thread_id = current_setting('test.loop0')::uuid AND recipient = 'owner') <> 1 THEN
        RAISE EXCEPTION 'FAIL C09: loop guard ok=% paused=%', v_ok, v_paused;
    END IF;
    v_res := app.claim_chat_messages(current_setting('test.ws')::uuid, (SELECT id FROM chats WHERE session_id = 'loop-7'), false);
    IF jsonb_array_length(v_res->'messages') <> 1 THEN
        RAISE EXCEPTION 'FAIL C09: held message not delivered on the next turn';
    END IF;
    RAISE NOTICE 'PASS C09 after 6 automatic wakes in a thread the next one is held for the chat''s next turn and the owner is told once';
END $$;
COMMIT;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.send_message(:'ws', gen_random_uuid(), 'claude', NULL, 'Go on', :'loop0', '[]') AS owner_msg \gset
COMMIT;
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.send_message(:'ws', gen_random_uuid(), 'claude', NULL, 'after owner', :'loop0', '[]', NULL, :'chat_b') AS after_owner \gset
COMMIT;
SELECT set_config('test.after_owner', :'after_owner', false) \gset
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    IF NOT (app.claim_chat_messages(current_setting('test.ws')::uuid, current_setting('test.b')::uuid, true)->'messages')
           @> jsonb_build_array(jsonb_build_object('id', current_setting('test.after_owner'))) THEN
        RAISE EXCEPTION 'FAIL C10: owner message did not reset the loop guard';
    END IF;
    RAISE NOTICE 'PASS C10 a message from the owner in the thread lets automatic wakes continue';
END $$;
COMMIT;

-- Addressing: an assistant writing to one key with several chats must name one;
-- with exactly one chat the message goes to it; the owner may still write to a key.
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3c', 32), 'hex'), 'files:read') IS NOT NULL AS ok \gset
SELECT app.upsert_chat(:'ws', 'opencode', 'mcp-77-1795000000', NULL) AS chat_r \gset
COMMIT;
SELECT set_config('test.r', :'chat_r', false) \gset
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('3a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$
DECLARE v_m uuid;
BEGIN
    BEGIN
        PERFORM app.send_message(current_setting('test.ws')::uuid, gen_random_uuid(), 'codex', NULL, 'which chat?', NULL, '[]',
            current_setting('test.a')::uuid, NULL);
        RAISE EXCEPTION 'FAIL C11: ambiguous key send accepted';
    EXCEPTION WHEN SQLSTATE 'AD423' THEN NULL;
    END;
    v_m := app.send_message(current_setting('test.ws')::uuid, gen_random_uuid(), 'reader', NULL, 'only one chat', NULL, '[]',
        current_setting('test.a')::uuid, NULL);
    IF (SELECT to_chat FROM messages WHERE id = v_m) IS DISTINCT FROM current_setting('test.r')::uuid THEN
        RAISE EXCEPTION 'FAIL C11: single chat not targeted';
    END IF;
    RAISE NOTICE 'PASS C11 a message to a key with several chats must name one (refused with AD423); a key with one chat gets it in that chat';
END $$;
COMMIT;

\echo 'CHAT MESSAGE CHECKS PASSED'
