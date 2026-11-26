-- docs/SECURITY-REVIEW-2026-11.md N1 and N2. Run as the cluster superuser on
-- a database migrated through the latest migration: setup that the app role
-- cannot do (a second owner, resetting the wake rate limit) runs as the
-- superuser, everything under test runs as aidrive_app.
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SET ROLE aidrive_app;
SELECT app.register_user('sr-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('sr-partner@example.test', :'hash') AS partner_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Security review') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
SELECT app.issue_agent_grant(:'ws', 'd5000000-0000-4000-8000-0000000000a1', 'Owner Claude', decode(repeat('9a', 32), 'hex'), 'files:write');
SELECT app.issue_agent_grant(:'ws', 'd5000000-0000-4000-8000-0000000000a2', 'Owner Codex', decode(repeat('9b', 32), 'hex'), 'files:write');
COMMIT;
RESET ROLE;
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'partner_id', 'owner');
SET ROLE aidrive_app;
BEGIN; SELECT set_config('app.user_id', :'partner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.issue_agent_grant(:'ws', 'd5000000-0000-4000-8000-0000000000b1', 'Partner Claude', decode(repeat('9c', 32), 'hex'), 'files:write');
COMMIT;
SELECT set_config('test.ws', :'ws', false) \gset

BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('9a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.upsert_chat(:'ws', 'claude', 'owner-session-1', 'C:\proj') AS chat_owner \gset
COMMIT;
SELECT set_config('test.chat_owner', :'chat_owner', false) \gset

-- N1: another person's key registers the same session id (listed to every
-- viewer by workspace_chats) and took the chat over with its messages.
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('9c', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    PERFORM app.upsert_chat(current_setting('test.ws')::uuid, 'claude', 'owner-session-1', NULL);
    RAISE EXCEPTION 'FAIL SR01: another person''s key took over the chat';
EXCEPTION WHEN SQLSTATE 'AD409' THEN
    RAISE NOTICE 'PASS SR01 a key cannot take over a chat registered by another person''s key';
END $$;
ROLLBACK;
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('9b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ DECLARE v_chat uuid; v_agent uuid; BEGIN
    v_chat := app.upsert_chat(current_setting('test.ws')::uuid, 'claude', 'owner-session-1', NULL);
    SELECT agent_id INTO v_agent FROM chats WHERE id = current_setting('test.chat_owner')::uuid;
    IF v_chat IS DISTINCT FROM current_setting('test.chat_owner')::uuid OR v_agent IS DISTINCT FROM 'd5000000-0000-4000-8000-0000000000a2' THEN
        RAISE EXCEPTION 'FAIL SR02: rebinding a folder to another key of the same person (chat %, key %)', v_chat, v_agent;
    END IF;
    RAISE NOTICE 'PASS SR02 a chat moves to another key of the same person (a folder rebound to a new profile)';
END $$;
ROLLBACK;

-- N2: the 6-wake guard counts one thread; an assistant that starts a new
-- thread every time woke the same chat without limit (only 30 s apart).
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('9b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.upsert_chat(:'ws', 'codex', 'codex-loop', NULL) AS chat_codex \gset
COMMIT;
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('9b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT count(app.send_message(:'ws', ('d6000000-0000-4000-8000-0000000000' || lpad(n::text, 2, '0'))::uuid,
                              'Owner Claude', NULL, 'new thread ' || n, NULL, '[]', :'chat_codex', :'chat_owner')) AS sent
FROM generate_series(1, 20) n \gset
COMMIT;
SELECT set_config('test.woken', '0', false) \gset
\ir security_review_wake.sql
\ir security_review_wake.sql
\ir security_review_wake.sql
\ir security_review_wake.sql
RESET ROLE;
DO $$ BEGIN
    IF current_setting('test.woken')::int <> 12 THEN
        RAISE EXCEPTION 'FAIL SR03: % automatic wakes into one chat without the owner (expected 12)', current_setting('test.woken');
    END IF;
    IF NOT EXISTS (SELECT 1 FROM messages WHERE workspace_id = current_setting('test.ws')::uuid AND recipient = 'owner'
                   AND subject = 'Automatic wakes paused for a chat') THEN
        RAISE EXCEPTION 'FAIL SR03: the owner was not told';
    END IF;
    RAISE NOTICE 'PASS SR03 at most 12 automatic wakes per chat across threads until the owner writes; the owner is told';
END $$;
SET ROLE aidrive_app;
BEGIN; SELECT app.begin_agent_request(:'ws', decode(repeat('9a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    IF jsonb_array_length(app.claim_chat_messages(current_setting('test.ws')::uuid, current_setting('test.chat_owner')::uuid, false)->'messages') <> 5 THEN
        RAISE EXCEPTION 'FAIL SR04: paused messages must still arrive on the chat''s next turn';
    END IF;
    RAISE NOTICE 'PASS SR04 messages held back by the pause arrive on the chat''s next turn';
END $$;
ROLLBACK;
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.send_message(:'ws', 'd7000000-0000-4000-8000-000000000001', 'assistants', NULL, 'carry on', NULL, '[]') AS resume \gset
COMMIT;
\ir security_review_wake.sql
DO $$ BEGIN
    IF current_setting('test.woken')::int <= 12 THEN
        RAISE EXCEPTION 'FAIL SR05: the owner writing did not lift the pause';
    END IF;
    RAISE NOTICE 'PASS SR05 a message from the owner lifts the pause';
END $$;
RESET ROLE;
