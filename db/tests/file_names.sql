-- Run as aidrive_app on a database migrated through 0024 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('names-owner@example.test', :'hash') AS owner_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Names one') AS ws \gset
SELECT app.create_workspace('Names two') AS ws2 \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'create', NULL, NULL, 'plan.md', 'v1', NULL);
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.ws2', :'ws2', false), set_config('test.owner', :'owner_id', false) \gset

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.write_text_file(current_setting('test.ws')::uuid, gen_random_uuid(), 'create', NULL, NULL, 'plan.md', 'v2', NULL);
    RAISE EXCEPTION 'FAIL N01: duplicate name created';
EXCEPTION WHEN SQLSTATE 'AD413' THEN RAISE NOTICE 'PASS N01 a second file with the same name is refused';
END $$;
DO $$ BEGIN
    PERFORM app.write_text_file(current_setting('test.ws')::uuid, gen_random_uuid(), 'create', NULL, NULL, 'Plan.MD', 'v2', NULL);
    RAISE EXCEPTION 'FAIL N02: name differing only in case created';
EXCEPTION WHEN SQLSTATE 'AD413' THEN RAISE NOTICE 'PASS N02 names are compared without case';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws2', true) \gset
SELECT app.write_text_file(:'ws2', gen_random_uuid(), 'create', NULL, NULL, 'plan.md', 'other space', NULL) IS NOT NULL AS ok \gset
COMMIT;
\if :ok
\echo 'PASS N03 the same name is allowed in another space'
\endif

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.write_text_file(:'ws', '64000000-0000-4000-8000-000000000001', 'create', NULL, NULL, 'notes.md', 'n', NULL) AS first \gset
SELECT app.write_text_file(:'ws', '64000000-0000-4000-8000-000000000001', 'create', NULL, NULL, 'notes.md', 'n', NULL) AS again \gset
COMMIT;
SELECT :'first' = :'again' AS same \gset
\if :same
\echo 'PASS N04 retrying a create with the same request_id still returns the first version'
\else
\echo 'FAIL N04 retry did not return the first version'
SELECT 1/0;
\endif
