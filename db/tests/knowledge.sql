-- Run as aidrive_app on a database migrated through 0012 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('k-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('k-editor@example.test', :'hash') AS editor_id \gset
SELECT app.register_user('k-viewer@example.test', :'hash') AS viewer_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Knowledge') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'editor_id', 'editor'), (:'ws', :'viewer_id', 'viewer');
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'create', NULL, NULL, 'pricing.md', 'Flat price for solo founders', NULL) AS v1 \gset
SELECT file_id FROM file_versions WHERE id = :'v1' \gset
SELECT app.issue_agent_grant(:'ws', '90000000-0000-4000-8000-0000000000e1', 'Claude', decode(repeat('e7', 32), 'hex'), 'files:write');
SELECT app.upsert_session(:'ws', 'codex', 'k-session', 'k', 'import', NULL, NULL) AS sid \gset
SELECT app.add_session_events(:'ws', :'sid', '[{"key":"e1","ordinal":0,"role":"user","text":"We decided against per-seat pricing"}]'::jsonb, true);
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.file', :'file_id', false), set_config('test.v1', :'v1', false), set_config('test.sid', :'sid', false) \gset

BEGIN; SELECT set_config('app.user_id', :'viewer_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.propose_item(current_setting('test.ws')::uuid, gen_random_uuid(), 'fact', NULL, 'x', NULL, NULL, '[]');
    RAISE EXCEPTION 'FAIL K01: viewer proposed';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS K01 viewer cannot propose knowledge';
END $$;
ROLLBACK;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('e7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.propose_item(:'ws', 'a1000000-0000-4000-8000-000000000001', 'decision', ' Pricing ', 'Use a flat monthly price for solo founders', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1', 'quote', 'Flat price for solo founders'),
                      jsonb_build_object('session_id', :'sid', 'event_key', 'e1', 'quote', 'against per-seat pricing'))) AS k1 \gset
SELECT app.propose_item(:'ws', 'a1000000-0000-4000-8000-000000000001', 'decision', ' Pricing ', 'Use a flat monthly price for solo founders', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS k1_replay \gset
SELECT set_config('test.k1', :'k1', false), set_config('test.k1r', :'k1_replay', false) \gset
DO $$ BEGIN
    IF current_setting('test.k1') <> current_setting('test.k1r')
       OR (SELECT topic FROM knowledge_items WHERE id = current_setting('test.k1')::uuid) <> 'pricing'
       OR (SELECT agent_id FROM knowledge_items WHERE id = current_setting('test.k1')::uuid) <> '90000000-0000-4000-8000-0000000000e1'
       OR (SELECT count(*) FROM knowledge_evidence WHERE item_id = current_setting('test.k1')::uuid) <> 2 THEN
        RAISE EXCEPTION 'FAIL K02: proposal';
    END IF;
    RAISE NOTICE 'PASS K02 agent proposal with file and session evidence, normalized topic, attribution, idempotent replay';
END $$;
DO $$ BEGIN
    PERFORM app.propose_item(current_setting('test.ws')::uuid, gen_random_uuid(), 'attempt', NULL, 'Tried per-seat', NULL, NULL, '[]');
    RAISE EXCEPTION 'FAIL K03: attempt without outcome';
EXCEPTION WHEN check_violation THEN RAISE NOTICE 'PASS K03 attempts require an outcome';
END $$;
DO $$ BEGIN
    PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.k1')::uuid, 'approved', NULL, NULL);
    RAISE EXCEPTION 'FAIL K04: agent approved';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS K04 agents cannot approve knowledge';
END $$;
SELECT app.propose_item(:'ws', gen_random_uuid(), 'attempt', 'pricing', 'Per-seat pricing test confused solo users', 'failed', NULL, '[]') AS attempt_id \gset
COMMIT;

-- Evidence must belong to this workspace.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Other K') AS other_ws \gset
SELECT set_config('app.workspace_id', :'other_ws', true) \gset
SELECT app.upsert_session(:'other_ws', 'codex', 'other', 'o', 'import', NULL, NULL) AS osid \gset
SELECT app.add_session_events(:'other_ws', :'osid', '[{"key":"x","ordinal":0,"role":"user","text":"secret"}]'::jsonb, true);
SELECT set_config('app.workspace_id', :'ws', true), set_config('test.osid', :'osid', true) \gset
DO $$ BEGIN
    PERFORM app.propose_item(current_setting('test.ws')::uuid, gen_random_uuid(), 'fact', NULL, 'x', NULL, NULL,
        jsonb_build_array(jsonb_build_object('session_id', current_setting('test.osid'), 'event_key', 'x')));
    RAISE EXCEPTION 'FAIL K05: foreign session evidence';
EXCEPTION WHEN foreign_key_violation THEN RAISE NOTICE 'PASS K05 evidence from another workspace is refused';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.k1')::uuid, 'approved', NULL, NULL);
    RAISE EXCEPTION 'FAIL K06: editor approved';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS K06 editors cannot approve knowledge';
END $$;
ROLLBACK;

-- Owner edit-approve keeps the original wording as superseded history.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.decide_item(:'ws', :'k1', 'approved', 'Clarified', 'Use one flat monthly price for solo founders; no per-seat plans') AS k1_final \gset
SELECT set_config('test.k1f', :'k1_final', true) \gset
DO $$ BEGIN
    IF (SELECT status FROM knowledge_items WHERE id = current_setting('test.k1')::uuid) <> 'superseded'
       OR (SELECT status FROM knowledge_items WHERE id = current_setting('test.k1f')::uuid) <> 'approved'
       OR (SELECT count(*) FROM knowledge_evidence WHERE item_id = current_setting('test.k1f')::uuid) <> 2 THEN
        RAISE EXCEPTION 'FAIL K07: edit-approve';
    END IF;
    PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.k1')::uuid, 'approved', NULL, NULL);
    RAISE EXCEPTION 'FAIL K07: decided twice';
EXCEPTION WHEN SQLSTATE 'AD409' THEN RAISE NOTICE 'PASS K07 owner edit-approve creates a new approved revision with evidence; original kept as superseded; decisions are final';
END $$;
COMMIT;
SELECT set_config('test.k1f', :'k1_final', false) \gset

-- Correction of an approved item, and approval refused on changed evidence.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Flat price with an annual discount', NULL, :'k1_final',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS k2 \gset
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'update', :'file_id', :'v1', NULL, 'Flat price, annual discount 20%', NULL) AS v2 \gset
SELECT set_config('test.k2', :'k2', true) \gset
DO $$ BEGIN
    PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.k2')::uuid, 'approved', NULL, NULL);
    RAISE EXCEPTION 'FAIL K08: approved on changed evidence';
EXCEPTION WHEN SQLSTATE 'AD412' THEN RAISE NOTICE 'PASS K08 approval refused after cited file changed';
END $$;
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Flat price with a 20% annual discount', NULL, :'k1_final',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v2'))) AS k3 \gset
SELECT app.decide_item(:'ws', :'k3', 'approved', NULL, NULL);
SELECT set_config('test.k3', :'k3', true) \gset
DO $$ BEGIN
    IF (SELECT status FROM knowledge_items WHERE id = current_setting('test.k1f')::uuid) <> 'superseded'
       OR (SELECT count(*) FROM knowledge_items WHERE topic = 'pricing' AND status = 'approved' AND kind = 'decision') <> 1 THEN
        RAISE EXCEPTION 'FAIL K09: correction';
    END IF;
    PERFORM app.propose_item(current_setting('test.ws')::uuid, gen_random_uuid(), 'fact', NULL, 'x', NULL, current_setting('test.k2')::uuid, '[]');
    RAISE EXCEPTION 'FAIL K09: corrected a non-approved item';
EXCEPTION WHEN check_violation THEN RAISE NOTICE 'PASS K09 approved correction supersedes the old decision; only approved items can be corrected';
END $$;
DO $$ BEGIN
    UPDATE knowledge_items SET statement = 'rewritten';
    RAISE EXCEPTION 'FAIL K10: item rewritten';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS K10 knowledge items cannot be rewritten';
END $$;
DO $$ BEGIN
    IF (SELECT count(*) FROM knowledge_items WHERE search @@ to_tsquery('simple', 'discou:*')) < 1 THEN
        RAISE EXCEPTION 'FAIL K11: search';
    END IF;
    RAISE NOTICE 'PASS K11 knowledge is full-text searchable with prefixes';
END $$;
COMMIT;

-- Approving a handoff turns its decisions into approved items with evidence.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.record_handoff(:'ws', gen_random_uuid(), 'Set up interviews', ARRAY['Interview five founders first', 'No demo before questions'], 'Recruit',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', (SELECT current_version_id FROM files WHERE id = :'file_id')))) AS h \gset
SELECT app.decide_handoff(:'ws', :'h', 'approved', NULL);
SELECT set_config('test.h', :'h', true) \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM knowledge_items k JOIN knowledge_evidence e ON e.item_id = k.id
        WHERE k.status = 'approved' AND k.kind = 'decision' AND e.handoff_id = current_setting('test.h')::uuid) <> 2
       OR (SELECT count(*) FROM knowledge_items k JOIN knowledge_evidence e ON e.item_id = k.id
        WHERE k.statement = 'No demo before questions' AND e.version_id IS NOT NULL) <> 1 THEN
        RAISE EXCEPTION 'FAIL K12: handoff items';
    END IF;
    RAISE NOTICE 'PASS K12 handoff approval creates approved decision items citing the handoff and its file versions';
END $$;
SELECT app.delete_session(:'ws', :'sid');
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM knowledge_evidence WHERE session_id = current_setting('test.sid')::uuid) THEN
        RAISE EXCEPTION 'FAIL K13: session evidence survived deletion';
    END IF;
    RAISE NOTICE 'PASS K13 deleting a session removes the evidence that cited it';
END $$;
COMMIT;

\echo 'KNOWLEDGE CHECKS PASSED'
