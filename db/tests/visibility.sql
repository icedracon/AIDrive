-- Run as aidrive_app on a database migrated through 0015 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('r-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('r-editor@example.test', :'hash') AS editor_id \gset
SELECT app.register_user('r-viewer@example.test', :'hash') AS viewer_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Visibility') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'editor_id', 'editor'), (:'ws', :'viewer_id', 'viewer');
SELECT app.issue_agent_grant(:'ws', '90000000-0000-4000-8000-0000000000c1', 'Claude', decode(repeat('c7', 32), 'hex'), 'files:write');
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.owner', :'owner_id', false), set_config('test.editor', :'editor_id', false) \gset

-- The owner's assistant captures a session; the editor captures their own.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('c7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.upsert_session(:'ws', 'claude-code', 'owner-1', 'owner work', 'import', NULL, NULL) AS s1 \gset
SELECT app.add_session_events(:'ws', :'s1', '[{"key":"a","ordinal":0,"role":"user","text":"salary numbers for the hire"}]'::jsonb, true);
COMMIT;
BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.upsert_session(:'ws', 'codex', 'editor-1', 'editor work', 'import', NULL, NULL) AS s2 \gset
SELECT app.add_session_events(:'ws', :'s2', '[{"key":"b","ordinal":0,"role":"user","text":"my private draft"}]'::jsonb, true);
COMMIT;
SELECT set_config('test.s1', :'s1', false), set_config('test.s2', :'s2', false) \gset

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF (SELECT visibility FROM sessions WHERE id = current_setting('test.s1')::uuid) <> 'private'
       OR (SELECT count(*) FROM session_events WHERE session_id = current_setting('test.s1')::uuid) <> 1 THEN
        RAISE EXCEPTION 'FAIL R01: new session not private or owner cannot read it';
    END IF;
    IF EXISTS (SELECT 1 FROM sessions WHERE id = current_setting('test.s2')::uuid)
       OR EXISTS (SELECT 1 FROM session_events WHERE session_id = current_setting('test.s2')::uuid) THEN
        RAISE EXCEPTION 'FAIL R01: workspace owner sees the editor''s private session';
    END IF;
    RAISE NOTICE 'PASS R01 captured sessions are private: the creator reads them, the workspace owner does not see someone else''s';
END $$;
ROLLBACK;
BEGIN; SELECT set_config('app.user_id', :'viewer_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM sessions) OR EXISTS (SELECT 1 FROM session_events WHERE search @@ to_tsquery('simple', 'salary')) THEN
        RAISE EXCEPTION 'FAIL R02: viewer reads private sessions or their search entries';
    END IF;
    RAISE NOTICE 'PASS R02 other members see neither private sessions nor their events in search';
END $$;
ROLLBACK;
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('c7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM sessions) <> 1 THEN RAISE EXCEPTION 'FAIL R03: owner assistant visibility'; END IF;
    PERFORM app.set_session_visibility(current_setting('test.ws')::uuid, current_setting('test.s1')::uuid, 'team');
    RAISE EXCEPTION 'FAIL R03: assistant changed visibility';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS R03 the owner''s assistants read the owner''s private sessions but cannot share them';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    BEGIN
        PERFORM app.set_session_visibility(current_setting('test.ws')::uuid, current_setting('test.s1')::uuid, 'team');
        RAISE EXCEPTION 'FAIL R04: editor shared the owner''s session';
    EXCEPTION WHEN no_data_found THEN NULL;
    END;
    BEGIN
        PERFORM app.upsert_session(current_setting('test.ws')::uuid, 'claude-code', 'owner-1', 'hijack', 'import', NULL, NULL);
        RAISE EXCEPTION 'FAIL R04: editor merged into the owner''s session';
    EXCEPTION WHEN SQLSTATE 'AD409' THEN NULL;
    END;
    BEGIN
        PERFORM app.add_session_events(current_setting('test.ws')::uuid, current_setting('test.s1')::uuid, '[{"key":"z","ordinal":9,"role":"user","text":"x"}]'::jsonb, false);
        RAISE EXCEPTION 'FAIL R04: editor appended to the owner''s session';
    EXCEPTION WHEN no_data_found THEN NULL;
    END;
    PERFORM app.delete_session(current_setting('test.ws')::uuid, current_setting('test.s1')::uuid);
    RAISE EXCEPTION 'FAIL R04: editor deleted the owner''s session';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS R04 other members cannot share, join, append to or delete someone else''s session';
END $$;
DO $$ BEGIN
    PERFORM app.set_session_visibility(current_setting('test.ws')::uuid, current_setting('test.s2')::uuid, 'public');
    RAISE EXCEPTION 'FAIL R05: unknown visibility accepted';
EXCEPTION WHEN check_violation THEN RAISE NOTICE 'PASS R05 only private and team are accepted';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.set_session_visibility(:'ws', :'s1', 'team');
COMMIT;
BEGIN; SELECT set_config('app.user_id', :'viewer_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM session_events WHERE search @@ to_tsquery('simple', 'salary')) <> 1
       OR EXISTS (SELECT 1 FROM sessions WHERE id = current_setting('test.s2')::uuid) THEN
        RAISE EXCEPTION 'FAIL R06: sharing';
    END IF;
    RAISE NOTICE 'PASS R06 a shared session and its events become visible to the team; other private sessions stay hidden';
END $$;
ROLLBACK;

-- Private knowledge: proposed and decided by its creator only.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Team: flat price', NULL, NULL, '[]') AS t1 \gset
SELECT app.decide_item(:'ws', :'t1', 'approved', NULL, NULL);
COMMIT;
BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Mine: I quote annual first', NULL, NULL,
    jsonb_build_array(jsonb_build_object('session_id', :'s2', 'event_key', 'b', 'quote', 'my private draft')), 'private') AS p1 \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Mine: I never discount', NULL, NULL, '[]', 'private') AS p2 \gset
COMMIT;
SELECT set_config('test.t1', :'t1', false), set_config('test.p1', :'p1', false), set_config('test.p2', :'p2', false) \gset

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM knowledge_items WHERE id = current_setting('test.p1')::uuid)
       OR EXISTS (SELECT 1 FROM knowledge_evidence WHERE item_id = current_setting('test.p1')::uuid) THEN
        RAISE EXCEPTION 'FAIL R07: workspace owner sees a private item or its evidence';
    END IF;
    PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.p1')::uuid, 'approved', NULL, NULL);
    RAISE EXCEPTION 'FAIL R07: workspace owner decided a private item';
EXCEPTION WHEN no_data_found THEN RAISE NOTICE 'PASS R07 private knowledge and its evidence are invisible to the workspace owner, who cannot decide it';
END $$;
DO $$ BEGIN
    PERFORM app.retire_item(current_setting('test.ws')::uuid, current_setting('test.p1')::uuid, NULL);
    RAISE EXCEPTION 'FAIL R08: workspace owner retired a private item';
EXCEPTION WHEN no_data_found THEN RAISE NOTICE 'PASS R08 the workspace owner cannot retire someone else''s private item';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF cardinality(app.item_conflicts(current_setting('test.ws')::uuid, 'decision', 'pricing', NULL, current_setting('test.p1')::uuid)) <> 0 THEN
        RAISE EXCEPTION 'FAIL R09: a private proposal conflicts with team knowledge';
    END IF;
END $$;
SELECT app.decide_item(:'ws', :'p1', 'approved', NULL, NULL);
DO $$ BEGIN
    IF (SELECT status FROM knowledge_items WHERE id = current_setting('test.p1')::uuid) <> 'approved'
       OR (SELECT status FROM knowledge_items WHERE id = current_setting('test.t1')::uuid) <> 'approved' THEN
        RAISE EXCEPTION 'FAIL R09: private approval';
    END IF;
    PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.p2')::uuid, 'approved', NULL, NULL);
    RAISE EXCEPTION 'FAIL R09: same-slot private proposal approved without resolution';
EXCEPTION WHEN SQLSTATE 'AD410' THEN RAISE NOTICE 'PASS R09 the creator approves private knowledge; conflicts stay inside one person''s private items and never touch team knowledge';
END $$;
DO $$ BEGIN
    PERFORM app.propose_item(current_setting('test.ws')::uuid, gen_random_uuid(), 'decision', 'pricing', 'x', NULL,
        current_setting('test.t1')::uuid, '[]', 'private');
    RAISE EXCEPTION 'FAIL R10: private correction of team knowledge';
EXCEPTION WHEN check_violation THEN RAISE NOTICE 'PASS R10 private items cannot correct team knowledge';
END $$;
DO $$ BEGIN
    PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.t1')::uuid, 'rejected', NULL, NULL);
    RAISE EXCEPTION 'FAIL R11: editor decided team knowledge';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS R11 team knowledge still needs the workspace owner';
END $$;
COMMIT;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('c7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    PERFORM app.propose_item(current_setting('test.ws')::uuid, gen_random_uuid(), 'fact', NULL, 'x', NULL, NULL,
        jsonb_build_array(jsonb_build_object('session_id', current_setting('test.s2'), 'event_key', 'b')));
    RAISE EXCEPTION 'FAIL R12: cited a private session the proposer cannot see';
EXCEPTION WHEN foreign_key_violation THEN NULL;
END $$;
DO $$ BEGIN
    PERFORM app.propose_item(current_setting('test.ws')::uuid, gen_random_uuid(), 'decision', 'pricing', 'x', NULL,
        current_setting('test.p1')::uuid, '[]', 'team');
    RAISE EXCEPTION 'FAIL R12: team correction of a private item';
EXCEPTION WHEN check_violation THEN RAISE NOTICE 'PASS R12 assistants cannot cite private sessions they cannot see or correct someone''s private items';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.retire_item(:'ws', :'p1', 'Changed my mind');
SELECT app.delete_session(:'ws', :'s2') AS deleted \gset
DO $$ BEGIN
    IF (SELECT status FROM knowledge_items WHERE id = current_setting('test.p1')::uuid) <> 'retired'
       OR EXISTS (SELECT 1 FROM sessions WHERE id = current_setting('test.s2')::uuid) THEN
        RAISE EXCEPTION 'FAIL R13: creator retire/delete';
    END IF;
    RAISE NOTICE 'PASS R13 the creator retires their private item and deletes their own session';
END $$;
DO $$ BEGIN
    UPDATE sessions SET visibility = 'team';
    RAISE EXCEPTION 'FAIL R14: visibility rewritten directly';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS R14 the application role cannot change visibility directly';
END $$;
COMMIT;

\echo 'VISIBILITY CHECKS PASSED'
