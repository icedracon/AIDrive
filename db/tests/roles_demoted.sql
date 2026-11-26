-- Run as aidrive_app after db/tests/roles.sql, once db-check-private.sh has
-- moved ownership of "Roles" from its issuer to the editor.
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
-- owner_id and ws come from db-check-private.sh (-v): aidrive_app cannot read users.
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT k.id AS p5 FROM knowledge_items k WHERE k.workspace_id = :'ws' AND k.statement = 'Two founders' \gset
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.p5', :'p5', false) \gset
BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('7b', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
DO $$ BEGIN
    BEGIN
        PERFORM app.decide_item(current_setting('test.ws')::uuid, current_setting('test.p5')::uuid, 'approved', NULL, NULL);
        RAISE EXCEPTION 'FAIL Q06: a demoted issuer''s medium key approved';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    IF app.agent_rank(current_setting('test.ws')::uuid) <> 3 THEN RAISE EXCEPTION 'FAIL Q06: rank not capped'; END IF;
    RAISE NOTICE 'PASS Q06 a medium key acts as low once its issuer no longer owns the space';
END $$;
ROLLBACK;
