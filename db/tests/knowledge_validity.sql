-- Run as aidrive_app on a database migrated through 0013 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('v-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('v-editor@example.test', :'hash') AS editor_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Validity') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'editor_id', 'editor');
SELECT app.issue_agent_grant(:'ws', '90000000-0000-4000-8000-0000000000f1', 'Codex', decode(repeat('f7', 32), 'hex'), 'files:write');
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Flat monthly price', NULL, NULL, '[]') AS d1 \gset
SELECT app.decide_item(:'ws', :'d1', 'approved', NULL, NULL);
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'pricing', 'Competitors charge per seat', NULL, NULL, '[]') AS f1 \gset
SELECT app.decide_item(:'ws', :'f1', 'approved', NULL, NULL);
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.d1', :'d1', false), set_config('test.f1', :'f1', false) \gset

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM knowledge_items WHERE workspace_id = current_setting('test.ws')::uuid
        AND status = 'approved' AND valid_from = decided_at AND invalid_at IS NULL AND superseded_by IS NULL) <> 2 THEN
        RAISE EXCEPTION 'FAIL K14: approval did not open the validity interval';
    END IF;
    RAISE NOTICE 'PASS K14 approval opens the validity interval at the decision time';
END $$;
COMMIT;

-- An assistant proposes into occupied and empty slots.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('f7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'Pricing', 'Per-seat price for teams', NULL, NULL, '[]') AS p1 \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'attempt', 'pricing', 'Tried a free tier', 'failed', NULL, '[]') AS a1 \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', NULL, 'Ship weekly', NULL, NULL, '[]') AS n1 \gset
COMMIT;
SELECT set_config('test.p1', :'p1', false), set_config('test.a1', :'a1', false), set_config('test.n1', :'n1', false) \gset

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$
DECLARE ws uuid := current_setting('test.ws')::uuid;
BEGIN
    IF app.item_conflicts(ws, 'decision', 'pricing', NULL, current_setting('test.p1')::uuid) <> ARRAY[current_setting('test.d1')::uuid]
       OR cardinality(app.item_conflicts(ws, 'attempt', 'pricing', NULL, current_setting('test.a1')::uuid)) <> 0
       OR cardinality(app.item_conflicts(ws, 'decision', NULL, NULL, current_setting('test.n1')::uuid)) <> 0
       OR cardinality(app.item_conflicts(ws, 'decision', 'pricing', current_setting('test.d1')::uuid, current_setting('test.p1')::uuid)) <> 0 THEN
        RAISE EXCEPTION 'FAIL K15: conflict set';
    END IF;
    RAISE NOTICE 'PASS K15 a proposal conflicts only with approved items of the same kind and topic; attempts, untopiced items and the corrected item never count';
END $$;
DO $$ BEGIN
    PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.p1')::uuid, 'approved', NULL, NULL);
    RAISE EXCEPTION 'FAIL K16: conflicting approval without resolution';
EXCEPTION WHEN SQLSTATE 'AD410' THEN RAISE NOTICE 'PASS K16 approving a conflicting proposal needs an explicit owner resolution';
END $$;
DO $$ BEGIN
    PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.p1')::uuid, 'approved', NULL, NULL, 'merge');
    RAISE EXCEPTION 'FAIL K17: unknown resolution accepted';
EXCEPTION WHEN check_violation THEN RAISE NOTICE 'PASS K17 unknown resolutions are refused';
END $$;
SELECT app.decide_item(:'ws', :'p1', 'approved', 'Teams and solo differ', NULL, 'keep_both');
SELECT app.decide_item(:'ws', :'n1', 'approved', NULL, NULL);
DO $$ BEGIN
    IF (SELECT count(*) FROM knowledge_items WHERE workspace_id = current_setting('test.ws')::uuid
        AND kind = 'decision' AND topic = 'pricing' AND status = 'approved') <> 2 THEN
        RAISE EXCEPTION 'FAIL K18: keep_both';
    END IF;
    RAISE NOTICE 'PASS K18 keep_both keeps the conflicting items current next to the new one';
END $$;
COMMIT;
SELECT clock_timestamp() AS t_before_replace \gset
SELECT set_config('test.t_before', :'t_before_replace', false) \gset

-- Replace supersedes every conflicting item at once.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('f7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Usage-based price', NULL, NULL, '[]') AS p2 \gset
COMMIT;
SELECT set_config('test.p2', :'p2', false) \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.decide_item(:'ws', :'p2', 'approved', NULL, NULL, 'replace');
DO $$ BEGIN
    IF (SELECT count(*) FROM knowledge_items WHERE id IN (current_setting('test.d1')::uuid, current_setting('test.p1')::uuid)
        AND status = 'superseded' AND superseded_by = current_setting('test.p2')::uuid
        AND invalid_at > valid_from AND invalidated_by = current_setting('app.user_id')::uuid) <> 2
       OR (SELECT status FROM knowledge_items WHERE id = current_setting('test.f1')::uuid) <> 'approved' THEN
        RAISE EXCEPTION 'FAIL K19: replace';
    END IF;
    RAISE NOTICE 'PASS K19 replace supersedes every conflicting item, records who ended them and when, and leaves other kinds alone';
END $$;
DO $$
DECLARE t timestamptz := current_setting('test.t_before')::timestamptz;
BEGIN
    IF (SELECT array_agg(statement ORDER BY statement) FROM knowledge_items
        WHERE workspace_id = current_setting('test.ws')::uuid AND kind = 'decision' AND topic = 'pricing'
          AND valid_from <= t AND (invalid_at IS NULL OR invalid_at > t)) <> ARRAY['Flat monthly price', 'Per-seat price for teams']
       OR (SELECT array_agg(statement) FROM knowledge_items
        WHERE workspace_id = current_setting('test.ws')::uuid AND kind = 'decision' AND topic = 'pricing' AND status = 'approved') <> ARRAY['Usage-based price'] THEN
        RAISE EXCEPTION 'FAIL K20: as-of recall';
    END IF;
    RAISE NOTICE 'PASS K20 validity intervals answer what was approved at an earlier moment, while current recall shows only the replacement';
END $$;
COMMIT;

-- Two corrections of the same item: once the first is approved, the second
-- conflicts with the replacement instead of silently joining it.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('f7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Usage-based with a cap', NULL, :'p2', '[]') AS c1 \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Usage-based with a floor', NULL, :'p2', '[]') AS c2 \gset
COMMIT;
SELECT set_config('test.c2', :'c2', false) \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.decide_item(:'ws', :'c1', 'approved', NULL, NULL);
DO $$ BEGIN
    PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.c2')::uuid, 'approved', NULL, NULL);
    RAISE EXCEPTION 'FAIL K21: second correction joined silently';
EXCEPTION WHEN SQLSTATE 'AD410' THEN RAISE NOTICE 'PASS K21 a second correction of an already replaced item must resolve the conflict with the replacement';
END $$;
COMMIT;

-- Retiring ends an approved item without a replacement; owner only.
BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.retire_item(current_setting('test.ws')::uuid, current_setting('test.f1')::uuid, NULL);
    RAISE EXCEPTION 'FAIL K22: editor retired';
EXCEPTION WHEN insufficient_privilege THEN NULL;
END $$;
ROLLBACK;
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('f7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    PERFORM app.retire_item(current_setting('test.ws')::uuid, current_setting('test.f1')::uuid, NULL);
    RAISE EXCEPTION 'FAIL K22: agent retired';
EXCEPTION WHEN insufficient_privilege THEN NULL;
END $$;
ROLLBACK;
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.retire_item(:'ws', :'f1', 'Market moved');
DO $$ BEGIN
    IF (SELECT count(*) FROM knowledge_items WHERE id = current_setting('test.f1')::uuid AND status = 'retired'
        AND invalid_at >= valid_from AND invalidation_note = 'Market moved' AND superseded_by IS NULL) <> 1 THEN
        RAISE EXCEPTION 'FAIL K22: retire';
    END IF;
    BEGIN
        PERFORM app.retire_item(current_setting('test.ws')::uuid, current_setting('test.f1')::uuid, NULL);
        RAISE EXCEPTION 'FAIL K22: retired twice';
    EXCEPTION WHEN SQLSTATE 'AD409' THEN NULL;
    END;
    PERFORM app.retire_item(current_setting('test.ws')::uuid, current_setting('test.c2')::uuid, NULL);
    RAISE EXCEPTION 'FAIL K22: retired a proposal';
EXCEPTION WHEN SQLSTATE 'AD409' THEN RAISE NOTICE 'PASS K22 only the owner retires, only current approved items, once; the note and end time are kept';
END $$;
COMMIT;

-- Handoff decisions become valid at the handoff's approval.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.record_handoff(:'ws', gen_random_uuid(), 'Plan', ARRAY['Interview first'], 'Recruit', '[]'::jsonb) AS h \gset
SELECT app.decide_handoff(:'ws', :'h', 'approved', NULL);
SELECT set_config('test.h', :'h', true) \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM knowledge_items k JOIN handoffs h ON h.id = current_setting('test.h')::uuid
        WHERE k.workspace_id = h.workspace_id AND k.statement = 'Interview first' AND k.valid_from = h.decided_at) <> 1 THEN
        RAISE EXCEPTION 'FAIL K23: handoff item validity';
    END IF;
    RAISE NOTICE 'PASS K23 handoff decisions are valid from the handoff approval';
END $$;
DO $$ BEGIN
    UPDATE knowledge_items SET invalid_at = clock_timestamp();
    RAISE EXCEPTION 'FAIL K24: validity rewritten';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS K24 the application role cannot rewrite validity';
END $$;
COMMIT;

\echo 'KNOWLEDGE VALIDITY CHECKS PASSED'
