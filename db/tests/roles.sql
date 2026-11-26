-- Run as aidrive_app on a database migrated through 0021 (scripts/db-check-private.sh).
-- Keys: Low (files:write, 7a), Low2 (files:write, 7d), Medium (review:accept, 7b), Grand (grand, 7c).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('role-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('role-editor@example.test', :'hash') AS editor_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Roles') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'editor_id', 'editor');
SELECT app.set_review_mode(:'ws', 'strict');
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'create', NULL, NULL, 'plan.md', 'Ship weekly', NULL) AS v1 \gset
SELECT file_id FROM file_versions WHERE id = :'v1' \gset
SELECT app.issue_agent_grant(:'ws', '63000000-0000-4000-8000-0000000000a1', 'Low', decode(repeat('7a', 32), 'hex'), 'files:write');
SELECT app.issue_agent_grant(:'ws', '63000000-0000-4000-8000-0000000000a4', 'Low2', decode(repeat('7d', 32), 'hex'), 'files:write');
SELECT app.issue_agent_grant(:'ws', '63000000-0000-4000-8000-0000000000a2', 'Medium', decode(repeat('7b', 32), 'hex'), 'review:accept');
SELECT app.issue_agent_grant(:'ws', '63000000-0000-4000-8000-0000000000a3', 'Grand', decode(repeat('7c', 32), 'hex'), 'grand');
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.file', :'file_id', false), set_config('test.v1', :'v1', false) \gset

-- Q01: only the owner issues keys, never an editor or an assistant.
BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.issue_agent_grant(current_setting('test.ws')::uuid, gen_random_uuid(), 'X', decode(repeat('70', 32), 'hex'), 'grand');
    RAISE EXCEPTION 'FAIL Q01: editor issued a grand key';
EXCEPTION WHEN insufficient_privilege THEN NULL;
END $$;
ROLLBACK;
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('7c', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    PERFORM app.issue_agent_grant(current_setting('test.ws')::uuid, gen_random_uuid(), 'X', decode(repeat('71', 32), 'hex'), 'grand');
    RAISE EXCEPTION 'FAIL Q01: an assistant issued a key';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS Q01 only the owner issues keys and roles, never an editor or an assistant (not even grand)';
END $$;
ROLLBACK;

-- Proposals: Low proposes p1 and a handoff h1; Medium proposes p2.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('7a', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'cadence', 'We ship weekly', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS p1 \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'audience', 'Solo founders', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS p3 \gset
SELECT app.record_handoff(:'ws', gen_random_uuid(), 'Plan drafted', ARRAY['Weekly releases'], 'Review the plan',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS h1 \gset
DO $$ BEGIN
    PERFORM app.decide_item(current_setting('test.ws')::uuid, (SELECT id FROM knowledge_items WHERE statement = 'Solo founders'), 'approved', NULL, NULL);
    RAISE EXCEPTION 'FAIL Q02: low key approved';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS Q02 a low key proposes but cannot approve';
END $$;
COMMIT;
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('7b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'pricing', 'Flat price', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS p2 \gset
SELECT app.record_handoff(:'ws', gen_random_uuid(), 'Pricing drafted', ARRAY[]::text[], 'Review pricing',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS h2 \gset
COMMIT;
SELECT set_config('test.p1', :'p1', false), set_config('test.p2', :'p2', false), set_config('test.p3', :'p3', false),
       set_config('test.h1', :'h1', false), set_config('test.h2', :'h2', false) \gset

-- Q03: Medium approves others' proposals and handoffs, never its own; no grand actions.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('7b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ DECLARE ws uuid := current_setting('test.ws')::uuid; BEGIN
    PERFORM app.decide_item(ws, current_setting('test.p1')::uuid, 'approved', 'looks right', NULL);
    PERFORM app.decide_handoff(ws, current_setting('test.h1')::uuid, 'approved', NULL);
    BEGIN
        PERFORM app.decide_item(ws, current_setting('test.p2')::uuid, 'approved', NULL, NULL);
        RAISE EXCEPTION 'FAIL Q03: medium approved its own proposal';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    BEGIN
        PERFORM app.decide_handoff(ws, current_setting('test.h2')::uuid, 'approved', NULL);
        RAISE EXCEPTION 'FAIL Q03: medium approved its own handoff';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    BEGIN
        PERFORM app.retire_item(ws, current_setting('test.p1')::uuid, NULL);
        RAISE EXCEPTION 'FAIL Q03: medium retired';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    BEGIN
        PERFORM app.set_review_mode(ws, 'exceptions');
        RAISE EXCEPTION 'FAIL Q03: medium changed the review mode';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
END $$;
DO $$ BEGIN
    IF (SELECT status FROM knowledge_items WHERE id = current_setting('test.p1')::uuid) <> 'approved'
       OR (SELECT decided_agent FROM knowledge_items WHERE id = current_setting('test.p1')::uuid) <> '63000000-0000-4000-8000-0000000000a2'
       OR (SELECT status FROM handoffs WHERE id = current_setting('test.h1')::uuid) <> 'approved'
       OR (SELECT decided_agent FROM handoffs WHERE id = current_setting('test.h1')::uuid) <> '63000000-0000-4000-8000-0000000000a2'
       OR (SELECT decided_agent FROM knowledge_items WHERE statement = 'Weekly releases') <> '63000000-0000-4000-8000-0000000000a2' THEN
        RAISE EXCEPTION 'FAIL Q03: medium decisions not recorded';
    END IF;
    RAISE NOTICE 'PASS Q03 a medium key approves other actors'' proposals and handoffs (recorded as its decision), never its own, and cannot retire or change the review mode';
END $$;
COMMIT;

-- Q04: Grand accepts its own work, retires, changes the review mode.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('7c', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'stack', 'Rust and PostgreSQL', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS p4 \gset
SELECT set_config('test.p4', :'p4', false) \gset
DO $$ DECLARE ws uuid := current_setting('test.ws')::uuid; BEGIN
    PERFORM app.decide_item(ws, current_setting('test.p4')::uuid, 'approved', NULL, NULL);
    PERFORM app.decide_item(ws, current_setting('test.p2')::uuid, 'rejected', 'not yet', NULL);
    PERFORM app.retire_item(ws, current_setting('test.p1')::uuid, 'cadence changed');
    PERFORM app.decide_handoff(ws, current_setting('test.h2')::uuid, 'approved', NULL);
    IF app.set_review_mode(ws, 'exceptions') <> 'exceptions' THEN RAISE EXCEPTION 'FAIL Q04: review mode'; END IF;
    IF (SELECT status FROM knowledge_items WHERE id = current_setting('test.p4')::uuid) <> 'approved'
       OR (SELECT status FROM knowledge_items WHERE id = current_setting('test.p1')::uuid) <> 'retired'
       OR (SELECT acted_agent FROM knowledge_items WHERE id = current_setting('test.p1')::uuid) <> '63000000-0000-4000-8000-0000000000a3'
       OR (SELECT decided_agent FROM knowledge_items WHERE id = current_setting('test.p1')::uuid) <> '63000000-0000-4000-8000-0000000000a2' THEN
        RAISE EXCEPTION 'FAIL Q04: grand decisions';
    END IF;
    RAISE NOTICE 'PASS Q04 a grand key accepts its own proposals, rejects, retires, decides handoffs and changes the review mode';
END $$;
ROLLBACK;

-- Q07: medium keys still write files as assistants.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('7b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'create', NULL, NULL, 'medium.md', 'still writes', NULL) IS NOT NULL AS wrote \gset
DO $$ BEGIN
    IF (SELECT a.agent_id FROM file_actions a JOIN files f ON f.id = a.file_id WHERE f.name = 'medium.md') <> '63000000-0000-4000-8000-0000000000a2' THEN
        RAISE EXCEPTION 'FAIL Q07: medium write';
    END IF;
    RAISE NOTICE 'PASS Q07 medium and grand keys still write files as assistants';
END $$;
ROLLBACK;

-- Q05: no assistant decides private items; Q06: a demoted issuer's medium key is low.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'notes', 'Private owner note', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1')), 'private') AS priv \gset
COMMIT;
SELECT set_config('test.priv', :'priv', false) \gset
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('7c', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.priv')::uuid, 'approved', NULL, NULL);
    RAISE EXCEPTION 'FAIL Q05: grand decided a private item';
EXCEPTION WHEN no_data_found THEN RAISE NOTICE 'PASS Q05 no assistant decides private items, not even grand';
END $$;
ROLLBACK;
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('7d', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'team', 'Two founders', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS p5 \gset
COMMIT;
SELECT set_config('test.p5', :'p5', false) \gset
