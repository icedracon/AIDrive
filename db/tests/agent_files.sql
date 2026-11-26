-- Run as aidrive_app on a database migrated through 0007 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('af-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('af-other@example.test', :'hash') AS other_id \gset
BEGIN; SELECT set_config('app.user_id', :'other_id', true) \gset
SELECT app.create_workspace('Other space') AS other_ws \gset
SELECT set_config('app.workspace_id', :'other_ws', true) \gset
SELECT app.issue_agent_grant(:'other_ws', gen_random_uuid(), 'Other writer', decode(repeat('e1', 32), 'hex'), 'files:write');
COMMIT;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Agent files') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'create', NULL, NULL, 'brief.md', 'v1', NULL) AS v1 \gset
SELECT app.issue_agent_grant(:'ws', '10000000-0000-4000-8000-0000000000a1', 'Metadata only', decode(repeat('a1', 32), 'hex'), 'workspace:read');
SELECT app.issue_agent_grant(:'ws', '10000000-0000-4000-8000-0000000000a2', 'Reader', decode(repeat('a2', 32), 'hex'), 'files:read');
SELECT app.issue_agent_grant(:'ws', '10000000-0000-4000-8000-0000000000a3', 'Writer', decode(repeat('a3', 32), 'hex'), 'files:write');
COMMIT;
SELECT set_config('test.ws', :'ws', false), set_config('test.v1', :'v1', false), set_config('test.owner', :'owner_id', false) \gset

DO $$ BEGIN
    PERFORM app.issue_agent_grant(gen_random_uuid(), gen_random_uuid(), 'x', decode(repeat('a4', 32), 'hex'), 'files:admin');
    RAISE EXCEPTION 'FAIL F01: grant issued without owner context';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS F01 grants need an owner context';
END $$;

BEGIN;
SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.issue_agent_grant(current_setting('test.ws')::uuid, gen_random_uuid(), 'Bad', decode(repeat('a5', 32), 'hex'), 'files:admin');
    RAISE EXCEPTION 'FAIL F02: unknown scope accepted';
EXCEPTION WHEN check_violation THEN RAISE NOTICE 'PASS F02 unknown scope rejected';
END $$;
ROLLBACK;

DO $$ BEGIN
    IF app.begin_agent_request(current_setting('test.ws')::uuid, decode(repeat('ff', 32), 'hex'), 'files:read') IS NOT NULL THEN
        RAISE EXCEPTION 'FAIL F03: unknown token accepted';
    END IF;
    IF app.begin_agent_request(current_setting('test.ws')::uuid, decode(repeat('e1', 32), 'hex'), 'files:read') IS NOT NULL THEN
        RAISE EXCEPTION 'FAIL F03: other workspace token accepted';
    END IF;
    RAISE NOTICE 'PASS F03 unknown and foreign-workspace tokens give no context';
END $$;

BEGIN;
DO $$ BEGIN
    PERFORM app.begin_agent_request(current_setting('test.ws')::uuid, decode(repeat('a1', 32), 'hex'), 'files:read');
    RAISE EXCEPTION 'FAIL F04: metadata grant read files';
EXCEPTION WHEN SQLSTATE 'AD403' THEN RAISE NOTICE 'PASS F04 workspace:read grant cannot read files';
END $$;
ROLLBACK;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('a2', 32), 'hex'), 'files:read') IS NOT NULL AS reader_ok \gset
DO $$ BEGIN
    IF current_setting('app.user_id') <> current_setting('test.owner')
       OR (SELECT count(*) FROM files WHERE current_version_id IS NOT NULL) <> 1
       OR (SELECT content FROM file_versions WHERE id = current_setting('test.v1')::uuid) <> 'v1' THEN
        RAISE EXCEPTION 'FAIL F05: reader context';
    END IF;
    RAISE NOTICE 'PASS F05 files:read grant reads current files through RLS';
END $$;
ROLLBACK;

BEGIN;
DO $$ BEGIN
    PERFORM app.begin_agent_request(current_setting('test.ws')::uuid, decode(repeat('a2', 32), 'hex'), 'files:write');
    RAISE EXCEPTION 'FAIL F06: reader got write context';
EXCEPTION WHEN SQLSTATE 'AD403' THEN RAISE NOTICE 'PASS F06 files:read grant cannot open a write context';
END $$;
ROLLBACK;

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('a3', 32), 'hex'), 'files:write') AS writer \gset
SELECT file_id FROM file_versions WHERE id = :'v1' \gset
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'update', :'file_id', :'v1', NULL, 'v2 by agent', NULL) AS v2 \gset
COMMIT;
SELECT set_config('test.v2', :'v2', false), set_config('test.file', :'file_id', false) \gset
BEGIN;
SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM file_versions v JOIN file_actions a ON a.version_id = v.id
                   WHERE v.id = current_setting('test.v2')::uuid
                     AND v.agent_id = '10000000-0000-4000-8000-0000000000a3' AND a.agent_id = v.agent_id
                     AND v.actor_id = current_setting('test.owner')::uuid) THEN
        RAISE EXCEPTION 'FAIL F07: agent attribution';
    END IF;
    RAISE NOTICE 'PASS F07 agent write recorded as issuer plus agent_id on version and action';
END $$;
SELECT app.write_text_file(:'ws', gen_random_uuid(), 'update', :'file_id', :'v2', NULL, 'v3 by owner', NULL) AS v3 \gset
DO $$ BEGIN
    IF (SELECT agent_id FROM file_versions WHERE revision = 3 AND file_id = current_setting('test.file')::uuid) IS NOT NULL THEN
        RAISE EXCEPTION 'FAIL F08: human write attributed to agent';
    END IF;
    RAISE NOTICE 'PASS F08 human write has no agent attribution';
END $$;
COMMIT;

-- A forged app.agent_id is refused unless it names this user's active writer grant.
BEGIN;
SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true),
       set_config('app.agent_id', '10000000-0000-4000-8000-0000000000a2', true) \gset
DO $$ BEGIN
    PERFORM app.write_text_file(current_setting('test.ws')::uuid, gen_random_uuid(), 'create', NULL, NULL, 'forged.md', 'x', NULL);
    RAISE EXCEPTION 'FAIL F09: read grant used for write attribution';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS F09 forged agent attribution refused';
END $$;
ROLLBACK;

-- Revocation blocks the next request.
BEGIN;
SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.revoke_agent_grant(:'ws', '10000000-0000-4000-8000-0000000000a3');
COMMIT;
DO $$ BEGIN
    IF app.begin_agent_request(current_setting('test.ws')::uuid, decode(repeat('a3', 32), 'hex'), 'files:write') IS NOT NULL THEN
        RAISE EXCEPTION 'FAIL F10: revoked grant still opens a context';
    END IF;
    RAISE NOTICE 'PASS F10 revoked writer gets no context';
END $$;

\echo 'AGENT FILE CHECKS PASSED'
