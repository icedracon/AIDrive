-- Run as aidrive_app on a database migrated through 0010 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('p-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('p-viewer@example.test', :'hash') AS viewer_id \gset
SELECT app.register_user('p-guesser@example.test', :'hash') AS guesser_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Pairing') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws', :'viewer_id', 'viewer');
COMMIT;
SELECT set_config('test.ws', :'ws', false) \gset

SELECT app.start_pairing('60000000-0000-4000-8000-000000000001', 'Codex desktop', decode(repeat('f1', 32), 'hex')) AS code1 \gset
SELECT app.start_pairing('60000000-0000-4000-8000-000000000001', 'Codex desktop', decode(repeat('f1', 32), 'hex')) AS code1_replay \gset
SELECT set_config('test.code1', :'code1', false), set_config('test.code1r', :'code1_replay', false) \gset
DO $$ BEGIN
    IF current_setting('test.code1') !~ '^[BCDFGHJKLMNPQRSTVWXZ]{8}$' OR current_setting('test.code1') <> current_setting('test.code1r') THEN
        RAISE EXCEPTION 'FAIL P01: code shape or replay';
    END IF;
    PERFORM app.start_pairing('60000000-0000-4000-8000-000000000001', 'Codex desktop', decode(repeat('f2', 32), 'hex'));
    RAISE EXCEPTION 'FAIL P01: same request ID with another secret accepted';
EXCEPTION WHEN SQLSTATE 'AD409' THEN RAISE NOTICE 'PASS P01 8-letter consonant code; replay returns it; reuse with another secret refused';
END $$;

DO $$ BEGIN
    PERFORM app.lookup_pairing(current_setting('test.code1'));
    RAISE EXCEPTION 'FAIL P02: anonymous lookup';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS P02 code lookup requires a signed-in user';
END $$;

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM app.pairing_status('60000000-0000-4000-8000-000000000001', decode(repeat('f9', 32), 'hex')))
       OR (SELECT status FROM app.pairing_status('60000000-0000-4000-8000-000000000001', decode(repeat('f1', 32), 'hex'))) <> 'pending' THEN
        RAISE EXCEPTION 'FAIL P03: status disclosure';
    END IF;
    RAISE NOTICE 'PASS P03 status needs the client secret; shows pending';
END $$;

BEGIN; SELECT set_config('app.user_id', :'viewer_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    IF (SELECT client_name FROM app.lookup_pairing(lower(substr(current_setting('test.code1'), 1, 4)) || '-' || substr(current_setting('test.code1'), 5))) <> 'Codex desktop' THEN
        RAISE EXCEPTION 'FAIL P04: normalized lookup';
    END IF;
    PERFORM app.decide_pairing(current_setting('test.code1'), current_setting('test.ws')::uuid, 'files:write', 'approved');
    RAISE EXCEPTION 'FAIL P04: viewer approved a pairing';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS P04 lowercase/dashed code resolves; non-owner cannot approve';
END $$;
ROLLBACK;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.decide_pairing(:'code1', :'ws', 'files:write', 'approved') AS grant_id \gset
COMMIT;
SELECT set_config('test.grant', :'grant_id', false) \gset
DO $$ DECLARE r record; BEGIN
    SELECT * INTO r FROM app.pairing_status('60000000-0000-4000-8000-000000000001', decode(repeat('f1', 32), 'hex'));
    IF r.status <> 'approved' OR r.workspace_name <> 'Pairing' OR r.scope <> 'files:write' OR r.approver_email <> 'p-owner@example.test' THEN
        RAISE EXCEPTION 'FAIL P05: status after approval %', r;
    END IF;
    IF app.begin_agent_request(current_setting('test.ws')::uuid, decode(repeat('f1', 32), 'hex'), 'files:write')::text <> current_setting('test.grant') THEN
        RAISE EXCEPTION 'FAIL P05: paired secret does not work as an agent key';
    END IF;
    RAISE NOTICE 'PASS P05 owner approval issues an agent grant for the client hash; client sees workspace, scope, approver';
END $$;

BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
DO $$ BEGIN
    PERFORM app.decide_pairing(current_setting('test.code1'), current_setting('test.ws')::uuid, 'files:read', 'approved');
    RAISE EXCEPTION 'FAIL P06: decided twice';
EXCEPTION WHEN SQLSTATE 'AD409' THEN RAISE NOTICE 'PASS P06 a pairing is decided once';
END $$;
ROLLBACK;

SELECT app.start_pairing('60000000-0000-4000-8000-000000000002', 'Claude Code', decode(repeat('f3', 32), 'hex')) AS code2 \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws', true) \gset
SELECT app.decide_pairing(:'code2', :'ws', 'files:write', 'denied') AS denied \gset
COMMIT;
DO $$ BEGIN
    IF (SELECT status FROM app.pairing_status('60000000-0000-4000-8000-000000000002', decode(repeat('f3', 32), 'hex'))) <> 'denied'
       OR app.begin_agent_request(current_setting('test.ws')::uuid, decode(repeat('f3', 32), 'hex'), 'workspace:read') IS NOT NULL THEN
        RAISE EXCEPTION 'FAIL P07: denial';
    END IF;
    RAISE NOTICE 'PASS P07 denied pairing creates no key';
END $$;

-- Guessing: failed attempts through the decision path still count, and the
-- eleventh lookup is refused even for a real code.
SELECT app.start_pairing('60000000-0000-4000-8000-000000000003', 'Target', decode(repeat('f4', 32), 'hex')) AS code3 \gset
SELECT set_config('test.code3', :'code3', false) \gset
BEGIN; SELECT set_config('app.user_id', :'guesser_id', true) \gset
SELECT app.create_workspace('Guesser space') AS gws \gset
COMMIT;
SELECT set_config('test.gws', :'gws', false) \gset
SELECT set_config('app.user_id', :'guesser_id', false) \gset
SELECT count(*) FILTER (WHERE r = 'not_found') AS misses
FROM (SELECT app.decide_pairing('BBBBBBBB', NULL, 'files:write', 'approved') AS r FROM generate_series(1, 10)) x \gset
SELECT set_config('test.misses', :'misses', false) \gset
DO $$ BEGIN
    IF current_setting('test.misses') <> '10' THEN RAISE EXCEPTION 'FAIL P08: misses %', current_setting('test.misses'); END IF;
    PERFORM app.decide_pairing(current_setting('test.code3'), current_setting('test.gws')::uuid, 'files:write', 'approved');
    RAISE EXCEPTION 'FAIL P08: guessing not limited';
EXCEPTION WHEN SQLSTATE 'AD429' THEN RAISE NOTICE 'PASS P08 ten failed codes block further attempts, including a correct one';
END $$;
SELECT set_config('app.user_id', '', false) \gset
DO $$ BEGIN
    IF (SELECT status FROM app.pairing_status('60000000-0000-4000-8000-000000000003', decode(repeat('f4', 32), 'hex'))) <> 'pending' THEN
        RAISE EXCEPTION 'FAIL P09: target pairing changed';
    END IF;
    RAISE NOTICE 'PASS P09 the targeted pairing stays pending';
END $$;

\echo 'PAIRING CHECKS PASSED'
