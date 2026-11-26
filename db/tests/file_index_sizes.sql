-- Run as aidrive_app on a database migrated through 0026 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('d-owner@example.test', :'hash') AS owner_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Disk index') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
SELECT app.issue_agent_grant(:'ws', '64000000-0000-4000-8000-0000000000a1', 'Claude', decode(repeat('6e', 32), 'hex'), 'files:write');
COMMIT;
SELECT set_config('test.ws', :'ws', false) \gset

BEGIN;
SELECT app.begin_agent_request(:'ws', decode(repeat('6e', 32), 'hex'), 'files:write') IS NOT NULL AS ok \gset
SELECT app.register_artifact(:'ws', 'C:\work\target', 'pc', 'C:\work', 'claude/x', 'Rust build output (rebuildable)',
                             clock_timestamp() + interval '7 days', 'build', 5000000000, 'session-1', true) AS t1 \gset
SELECT set_config('test.t1', :'t1', false) \gset
DO $$ DECLARE ws uuid := current_setting('test.ws')::uuid; r record; k timestamptz; BEGIN
    SELECT * INTO r FROM artifacts WHERE id = current_setting('test.t1')::uuid;
    IF r.kind <> 'build' OR r.size_bytes <> 5000000000 OR r.session_ref <> 'session-1' OR r.size_checked_at IS NULL OR r.last_seen_at IS NULL THEN
        RAISE EXCEPTION 'FAIL D01: first index';
    END IF;
    k := r.keep_until;
    -- The person extends it; an automatic re-index must not undo that.
    PERFORM app.resolve_artifact(ws, r.id, 'extend', clock_timestamp() + interval '30 days', 'still building');
    PERFORM app.register_artifact(ws, 'C:\work\target', 'pc', NULL, NULL, 'Rust build output (rebuildable)',
                                  clock_timestamp() + interval '7 days', 'build', 7000000000, 'session-2', true);
    SELECT * INTO r FROM artifacts WHERE id = current_setting('test.t1')::uuid;
    IF r.size_bytes <> 7000000000 OR r.keep_until < clock_timestamp() + interval '29 days' OR r.note <> 'still building'
       OR r.session_ref <> 'session-1' OR r.worktree <> 'C:\work' THEN
        RAISE EXCEPTION 'FAIL D02: refresh changed more than size';
    END IF;
    RAISE NOTICE 'PASS D01 build folders are indexed with kind, size and creating session';
    RAISE NOTICE 'PASS D02 an automatic re-index updates the size only; review date, note and creator stay';

    PERFORM app.resolve_artifact(ws, r.id, 'removed', NULL, 'freed 7 GB');
    PERFORM app.register_artifact(ws, 'C:\work\target', 'pc', NULL, NULL, 'Rust build output (rebuildable)',
                                  clock_timestamp() + interval '7 days', 'build', 1000, 'session-3', true);
    SELECT * INTO r FROM artifacts WHERE id = current_setting('test.t1')::uuid;
    IF r.status <> 'active' OR r.size_bytes <> 1000 OR r.keep_until > clock_timestamp() + interval '8 days' THEN
        RAISE EXCEPTION 'FAIL D03: rebuilt folder not re-registered';
    END IF;
    RAISE NOTICE 'PASS D03 a removed build folder that comes back is registered again with a fresh review date';

    -- Separate statements: a read in the same expression sees the old snapshot.
    IF app.record_artifact_checks(ws, jsonb_build_array(jsonb_build_object('id', r.id, 'exists', true, 'size_bytes', 2500))) <> 1 THEN
        RAISE EXCEPTION 'FAIL D04: check count';
    END IF;
    IF (SELECT size_bytes FROM artifacts WHERE id = r.id) <> 2500 THEN RAISE EXCEPTION 'FAIL D04: check size'; END IF;
    PERFORM app.resolve_artifact(ws, r.id, 'keep', NULL, NULL);
    PERFORM app.record_artifact_checks(ws, jsonb_build_array(jsonb_build_object('id', r.id, 'exists', false)));
    IF (SELECT status FROM artifacts WHERE id = r.id) <> 'missing' THEN RAISE EXCEPTION 'FAIL D04: kept file gone'; END IF;
    RAISE NOTICE 'PASS D04 checks record measured sizes, and a kept file found gone is recorded as missing';

    BEGIN
        PERFORM app.register_artifact(ws, 'C:\x', 'pc', NULL, NULL, 'x', NULL, 'cache', NULL, NULL, false);
        RAISE EXCEPTION 'FAIL D05: unknown kind';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    BEGIN
        PERFORM app.register_artifact(ws, 'C:\y', 'pc', NULL, NULL, 'y', NULL, 'temp', -1, NULL, false);
        RAISE EXCEPTION 'FAIL D05: negative size';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    IF (SELECT kind FROM app.workspace_artifacts(ws, 'all', false, NULL, 10) LIMIT 1) IS NULL THEN
        RAISE EXCEPTION 'FAIL D05: list lacks kind';
    END IF;
    RAISE NOTICE 'PASS D05 unknown kinds and negative sizes are refused; the list carries kind and size';
END $$;
COMMIT;
