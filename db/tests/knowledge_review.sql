-- Run as aidrive_app on a database migrated through 0017 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('x-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('x-editor@example.test', :'hash') AS editor_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Exceptions') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'editor_id', 'editor');
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'create', NULL, NULL, 'plan.md', 'Ship weekly', NULL) AS v1 \gset
SELECT file_id FROM file_versions WHERE id = :'v1' \gset
SELECT app.issue_agent_grant(:'ws', '90000000-0000-4000-8000-0000000000a1', 'Claude', decode(repeat('a7', 32), 'hex'), 'files:write');
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.file', :'file_id', false), set_config('test.v1', :'v1', false) \gset

-- A space without a setting is strict: nothing is accepted automatically.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('a7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'cadence', 'We ship weekly', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS s1 \gset
SELECT set_config('test.s1', :'s1', false) \gset
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM workspace_settings WHERE workspace_id = current_setting('test.ws')::uuid)
       OR (SELECT status FROM knowledge_items WHERE id = current_setting('test.s1')::uuid) <> 'proposed' THEN
        RAISE EXCEPTION 'FAIL E01: strict default';
    END IF;
    RAISE NOTICE 'PASS E01 a space without a review setting is strict and keeps proposals pending';
END $$;
DO $$ BEGIN
    PERFORM app.set_review_mode(current_setting('test.ws')::uuid, 'exceptions');
    RAISE EXCEPTION 'FAIL E02: agent changed review mode';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS E02 assistants cannot change the review mode';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.set_review_mode(current_setting('test.ws')::uuid, 'exceptions');
    RAISE EXCEPTION 'FAIL E03: editor changed review mode';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS E03 only the owner changes the review mode';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.set_review_mode(current_setting('test.ws')::uuid, 'lenient');
    RAISE EXCEPTION 'FAIL E04: unknown mode';
EXCEPTION WHEN check_violation THEN RAISE NOTICE 'PASS E04 unknown review modes are refused';
END $$;
ROLLBACK;
DO $$ BEGIN
    BEGIN
        INSERT INTO workspace_settings (workspace_id, review_mode, updated_by)
        VALUES (current_setting('test.ws')::uuid, 'exceptions', gen_random_uuid());
        RAISE EXCEPTION 'FAIL E05: app role wrote settings';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    BEGIN
        UPDATE knowledge_items SET auto_accepted = true;
        RAISE EXCEPTION 'FAIL E05: app role rewrote auto_accepted';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    RAISE NOTICE 'PASS E05 the app role cannot write review settings or acceptance flags directly';
END $$;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.set_review_mode(:'ws', 'exceptions');
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Flat monthly price', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS owner_item \gset
COMMIT;
SELECT set_config('test.owner_item', :'owner_item', false) \gset

-- The assistant proposes every shape once.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('a7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'audience', 'Pilot users are solo founders', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1', 'quote', 'Ship weekly'))) AS clean \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Per-seat price', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS conflicting \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'keystone', NULL, 'Never push without asking', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS keystone \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'Flat price, billed yearly', NULL, :'owner_item',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS correction \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'gotcha', NULL, 'The build needs WSL', NULL, NULL, '[]') AS bare \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'tooling', 'We use Postgres 17', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1')), 'private') AS private_item \gset
COMMIT;
SELECT set_config('test.clean', :'clean', false), set_config('test.conflicting', :'conflicting', false),
       set_config('test.keystone', :'keystone', false), set_config('test.correction', :'correction', false),
       set_config('test.bare', :'bare', false), set_config('test.private', :'private_item', false) \gset

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$
DECLARE r knowledge_items%ROWTYPE;
BEGIN
    SELECT * INTO r FROM knowledge_items WHERE id = current_setting('test.clean')::uuid;
    IF r.status <> 'approved' OR NOT r.auto_accepted OR r.valid_from IS NULL OR r.decided_by <> r.actor_id
       OR r.agent_id <> '90000000-0000-4000-8000-0000000000a1' OR r.confirmed_at IS NOT NULL THEN
        RAISE EXCEPTION 'FAIL E06: clean proposal % %', r.status, r.auto_accepted;
    END IF;
    IF NOT (SELECT auto_accepted FROM knowledge_items WHERE id = current_setting('test.owner_item')::uuid) THEN
        RAISE EXCEPTION 'FAIL E06: owner proposal not accepted';
    END IF;
    RAISE NOTICE 'PASS E06 a proposal with current evidence and no conflict is auto-accepted, flagged and attributed to its proposer';
END $$;
DO $$ BEGIN
    IF (SELECT status FROM knowledge_items WHERE id = current_setting('test.conflicting')::uuid) <> 'proposed'
       OR (SELECT status FROM knowledge_items WHERE id = current_setting('test.keystone')::uuid) <> 'proposed'
       OR (SELECT status FROM knowledge_items WHERE id = current_setting('test.correction')::uuid) <> 'proposed'
       OR (SELECT status FROM knowledge_items WHERE id = current_setting('test.bare')::uuid) <> 'proposed'
       OR EXISTS (SELECT 1 FROM knowledge_items WHERE id IN (current_setting('test.conflicting')::uuid, current_setting('test.keystone')::uuid,
                  current_setting('test.correction')::uuid, current_setting('test.bare')::uuid) AND auto_accepted) THEN
        RAISE EXCEPTION 'FAIL E07: an exception was accepted';
    END IF;
    RAISE NOTICE 'PASS E07 conflicts, keystones, corrections and unevidenced claims wait for the owner';
END $$;
COMMIT;

-- Stale evidence: the file moves on before the next proposal cites v1.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'update', :'file_id', :'v1', NULL, 'Ship every two weeks', NULL) AS v2 \gset
COMMIT;
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('a7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'cadence', 'We ship weekly', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS stale \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'cadence', 'We ship every two weeks', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v2'))) AS fresh \gset
COMMIT;
SELECT set_config('test.stale', :'stale', false), set_config('test.fresh', :'fresh', false), set_config('test.v2', :'v2', false) \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF (SELECT status FROM knowledge_items WHERE id = current_setting('test.stale')::uuid) <> 'proposed'
       OR (SELECT status FROM knowledge_items WHERE id = current_setting('test.fresh')::uuid) <> 'approved' THEN
        RAISE EXCEPTION 'FAIL E08: stale evidence';
    END IF;
    RAISE NOTICE 'PASS E08 a proposal citing an older file version waits; the same slot with current evidence is accepted';
END $$;
COMMIT;

-- The private item: auto-accepted for its creator, invisible to the owner.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM knowledge_items WHERE id = current_setting('test.private')::uuid) <> 1 THEN
        RAISE EXCEPTION 'FAIL E09: private item missing for its creator';
    END IF;
    RAISE NOTICE 'PASS E09 private proposals follow the same rule for their creator';
END $$;
COMMIT;

-- Keep and undo.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('a7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    PERFORM app.undo_item(current_setting('test.ws')::uuid, current_setting('test.clean')::uuid, NULL);
    RAISE EXCEPTION 'FAIL E10: agent undid';
EXCEPTION WHEN insufficient_privilege THEN NULL;
END $$;
DO $$ BEGIN
    PERFORM app.confirm_item(current_setting('test.ws')::uuid, current_setting('test.clean')::uuid);
    RAISE EXCEPTION 'FAIL E10: agent kept';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS E10 assistants can neither keep nor undo';
END $$;
ROLLBACK;
BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.undo_item(current_setting('test.ws')::uuid, current_setting('test.clean')::uuid, NULL);
    RAISE EXCEPTION 'FAIL E11: editor undid';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS E11 editors cannot undo team knowledge';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.undo_item(:'ws', :'clean', NULL);
SELECT app.confirm_item(:'ws', :'fresh');
DO $$
DECLARE r knowledge_items%ROWTYPE;
BEGIN
    SELECT * INTO r FROM knowledge_items WHERE id = current_setting('test.clean')::uuid;
    IF r.status <> 'retired' OR r.invalid_at IS NULL OR r.invalidation_note <> 'Undone by owner' THEN
        RAISE EXCEPTION 'FAIL E12: undo';
    END IF;
    SELECT * INTO r FROM knowledge_items WHERE id = current_setting('test.fresh')::uuid;
    IF r.status <> 'approved' OR r.confirmed_by IS NULL OR NOT r.auto_accepted THEN
        RAISE EXCEPTION 'FAIL E12: keep';
    END IF;
    RAISE NOTICE 'PASS E12 undo retires the item (history kept); keep records the owner check';
END $$;
DO $$ BEGIN
    PERFORM app.undo_item(current_setting('test.ws')::uuid, current_setting('test.fresh')::uuid, NULL);
    RAISE EXCEPTION 'FAIL E13: kept item undone';
EXCEPTION WHEN SQLSTATE 'AD409' THEN NULL;
END $$;
SELECT app.confirm_item(:'ws', :'owner_item');
DO $$ BEGIN
    PERFORM app.confirm_item(current_setting('test.ws')::uuid, current_setting('test.owner_item')::uuid);
    RAISE EXCEPTION 'FAIL E13: kept twice';
EXCEPTION WHEN SQLSTATE 'AD409' THEN NULL;
END $$;
DO $$ BEGIN
    PERFORM app.undo_item(current_setting('test.ws')::uuid, current_setting('test.conflicting')::uuid, NULL);
    RAISE EXCEPTION 'FAIL E13: proposal undone';
EXCEPTION WHEN SQLSTATE 'AD409' THEN RAISE NOTICE 'PASS E13 only unchecked auto-accepted items can be kept or undone, once';
END $$;
COMMIT;

-- Handoffs.
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('a7', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.record_handoff(:'ws', gen_random_uuid(), 'Drafted the plan', ARRAY['Weekly demos'], 'Write the script',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v2'))) AS h1 \gset
SELECT app.record_handoff(:'ws', gen_random_uuid(), 'Reviewed the plan', ARRAY['Demo on Fridays'], 'Book the room',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v2'))) AS h2 \gset
SELECT app.record_handoff(:'ws', gen_random_uuid(), 'Old notes', '{}', 'Nothing',
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v1'))) AS h_stale \gset
SELECT app.record_handoff(:'ws', gen_random_uuid(), 'No sources', '{}', 'Nothing', '[]') AS h_bare \gset
COMMIT;
SELECT set_config('test.h1', :'h1', false), set_config('test.h2', :'h2', false),
       set_config('test.h_stale', :'h_stale', false), set_config('test.h_bare', :'h_bare', false) \gset

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$
DECLARE r handoffs%ROWTYPE;
BEGIN
    SELECT * INTO r FROM handoffs WHERE id = current_setting('test.h2')::uuid;
    IF r.status <> 'approved' OR NOT r.auto_accepted OR r.replaced <> current_setting('test.h1')::uuid OR r.decided_by <> r.actor_id THEN
        RAISE EXCEPTION 'FAIL E14: auto handoff';
    END IF;
    IF (SELECT status FROM handoffs WHERE id = current_setting('test.h1')::uuid) <> 'superseded'
       OR (SELECT count(*) FROM handoffs WHERE workspace_id = current_setting('test.ws')::uuid AND status = 'approved') <> 1 THEN
        RAISE EXCEPTION 'FAIL E14: one approved handoff';
    END IF;
    IF (SELECT status FROM handoffs WHERE id = current_setting('test.h_stale')::uuid) <> 'proposed'
       OR (SELECT status FROM handoffs WHERE id = current_setting('test.h_bare')::uuid) <> 'proposed' THEN
        RAISE EXCEPTION 'FAIL E14: stale or unsourced handoff accepted';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM knowledge_items k JOIN knowledge_evidence e ON e.item_id = k.id
                   WHERE e.handoff_id = current_setting('test.h2')::uuid AND k.auto_accepted AND k.status = 'approved'
                     AND k.statement = 'Demo on Fridays') THEN
        RAISE EXCEPTION 'FAIL E14: derived decision not flagged';
    END IF;
    RAISE NOTICE 'PASS E14 handoffs with current sources are auto-accepted one at a time; stale and unsourced ones wait; derived decisions are flagged';
END $$;
SELECT app.undo_handoff(:'ws', :'h2', NULL);
DO $$ BEGIN
    IF (SELECT status FROM handoffs WHERE id = current_setting('test.h2')::uuid) <> 'rejected'
       OR (SELECT undone_at FROM handoffs WHERE id = current_setting('test.h2')::uuid) IS NULL
       OR (SELECT status FROM handoffs WHERE id = current_setting('test.h1')::uuid) <> 'approved'
       OR EXISTS (SELECT 1 FROM knowledge_items k JOIN knowledge_evidence e ON e.item_id = k.id
                  WHERE e.handoff_id = current_setting('test.h2')::uuid AND k.status = 'approved') THEN
        RAISE EXCEPTION 'FAIL E15: handoff undo';
    END IF;
    RAISE NOTICE 'PASS E15 undoing the current handoff rejects it, retires its decisions and restores the handoff it replaced';
END $$;
SELECT app.confirm_handoff(:'ws', :'h1');
DO $$ BEGIN
    PERFORM app.undo_handoff(current_setting('test.ws')::uuid, current_setting('test.h1')::uuid, NULL);
    RAISE EXCEPTION 'FAIL E16: kept handoff undone';
EXCEPTION WHEN SQLSTATE 'AD409' THEN NULL;
END $$;
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM knowledge_items k JOIN knowledge_evidence e ON e.item_id = k.id
               WHERE e.handoff_id = current_setting('test.h1')::uuid AND k.confirmed_at IS NULL) THEN
        RAISE EXCEPTION 'FAIL E16: keep did not cover derived decisions';
    END IF;
    RAISE NOTICE 'PASS E16 keeping a handoff also checks its decisions, and a kept handoff cannot be undone';
END $$;
-- The owner's own approval of a waiting handoff is not auto-accepted.
SELECT app.decide_handoff(:'ws', :'h_bare', 'approved', NULL);
DO $$ BEGIN
    IF (SELECT auto_accepted OR status <> 'approved' OR replaced <> current_setting('test.h1')::uuid
        FROM handoffs WHERE id = current_setting('test.h_bare')::uuid) THEN
        RAISE EXCEPTION 'FAIL E17: owner approval';
    END IF;
    RAISE NOTICE 'PASS E17 an owner approval stays owner-approved and records what it replaced';
END $$;
COMMIT;

-- Timeline and metric.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$
DECLARE kinds text[];
BEGIN
    SELECT array_agg(DISTINCT kind) INTO kinds FROM app.workspace_activity(current_setting('test.ws')::uuid, NULL, 100);
    IF NOT kinds @> ARRAY['knowledge.auto_accepted', 'knowledge.undone', 'knowledge.kept', 'knowledge.proposed',
                         'handoff.auto_accepted', 'handoff.undone', 'handoff.kept', 'handoff.approved'] THEN
        RAISE EXCEPTION 'FAIL E18: timeline kinds %', kinds;
    END IF;
    RAISE NOTICE 'PASS E18 the timeline separates auto-acceptance, owner approval, keep and undo';
END $$;
DO $$
DECLARE m record;
BEGIN
    SELECT sum(owner_approved) AS o, sum(auto_accepted) AS a, sum(kept) AS k, sum(undone) AS u, count(*) AS days
    INTO m FROM app.review_metrics(current_setting('test.ws')::uuid, 7);
    -- Auto: owner_item, clean, fresh, private (creator is the owner), h1, h2 = 6.
    -- Owner: h_bare = 1. Kept: fresh, owner_item, h1 = 3. Undone: clean, h2 = 2.
    IF m.days <> 7 OR m.a <> 6 OR m.o <> 1 OR m.k <> 3 OR m.u <> 2 THEN
        RAISE EXCEPTION 'FAIL E19: metrics days=% owner=% auto=% kept=% undone=%', m.days, m.o, m.a, m.k, m.u;
    END IF;
    RAISE NOTICE 'PASS E19 review metrics per day: owner approvals, auto-accepted, kept and undone';
END $$;
COMMIT;
BEGIN; SELECT set_config('app.user_id', :'editor_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$
DECLARE a bigint;
BEGIN
    SELECT sum(auto_accepted) INTO a FROM app.review_metrics(current_setting('test.ws')::uuid, 7);
    IF a <> 5 THEN RAISE EXCEPTION 'FAIL E20: editor sees % auto-accepted', a; END IF;
    IF EXISTS (SELECT 1 FROM app.workspace_activity(current_setting('test.ws')::uuid, NULL, 100) WHERE subject = 'We use Postgres 17') THEN
        RAISE EXCEPTION 'FAIL E20: private item in another member''s timeline';
    END IF;
    RAISE NOTICE 'PASS E20 metrics and timeline leave out another person''s private knowledge';
END $$;
COMMIT;

-- Switching back to strict stops auto-acceptance.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.set_review_mode(:'ws', 'strict');
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'hosting', 'Hosted on one VM', NULL, NULL,
    jsonb_build_array(jsonb_build_object('file_id', :'file_id', 'version_id', :'v2'))) AS strict_item \gset
SELECT set_config('test.strict_item', :'strict_item', false) \gset
DO $$ BEGIN
    IF (SELECT status FROM knowledge_items WHERE id = current_setting('test.strict_item')::uuid) <> 'proposed' THEN
        RAISE EXCEPTION 'FAIL E21: strict';
    END IF;
    RAISE NOTICE 'PASS E21 after switching back to strict, proposals wait for the owner again';
END $$;
ROLLBACK;
