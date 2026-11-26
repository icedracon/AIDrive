-- Run as aidrive_app on a database migrated through 0004 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('w-owner@example.test', :'hash') AS owner_id \gset
SELECT app.register_user('w-other@example.test', :'hash') AS other_id \gset

BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Owner one') AS ws_one \gset
SELECT app.create_workspace('Owner two') AS ws_two \gset
COMMIT;
BEGIN; SELECT set_config('app.user_id', :'other_id', true) \gset
SELECT app.create_workspace('Other only') AS ws_other \gset
COMMIT;

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM app.my_workspaces()) THEN
        RAISE EXCEPTION 'FAIL W01: workspaces listed without a user';
    END IF;
    RAISE NOTICE 'PASS W01 no user context lists nothing';
END $$;

BEGIN;
SELECT set_config('app.user_id', :'owner_id', true) \gset
DO $$ BEGIN
    IF (SELECT array_agg(name ORDER BY name) FROM app.my_workspaces()) IS DISTINCT FROM ARRAY['Owner one', 'Owner two']
       OR EXISTS (SELECT 1 FROM app.my_workspaces() WHERE role <> 'owner') THEN
        RAISE EXCEPTION 'FAIL W02: owner listing wrong';
    END IF;
    RAISE NOTICE 'PASS W02 user lists exactly own workspaces with role';
END $$;
COMMIT;

DO $$ BEGIN
    IF current_setting('app.user_id', true) IS DISTINCT FROM '' AND current_setting('app.user_id', true) IS NOT NULL THEN
        RAISE EXCEPTION 'FAIL W03: transaction-local user context survived commit';
    END IF;
    RAISE NOTICE 'PASS W03 user context does not survive the transaction';
END $$;

-- Sole owner cannot leave or demote themselves.
BEGIN;
SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws_one', true) \gset
DO $$ BEGIN
    DELETE FROM memberships WHERE user_id = app.current_user_id();
    RAISE EXCEPTION 'FAIL W04: last owner removed';
EXCEPTION WHEN integrity_constraint_violation THEN RAISE NOTICE 'PASS W04 last owner cannot be removed';
END $$;
DO $$ BEGIN
    UPDATE memberships SET role = 'editor' WHERE user_id = app.current_user_id();
    RAISE EXCEPTION 'FAIL W05: last owner demoted';
EXCEPTION WHEN integrity_constraint_violation THEN RAISE NOTICE 'PASS W05 last owner cannot be demoted';
END $$;
ROLLBACK;

-- With a second owner, handing over works; afterwards the new sole owner is protected.
BEGIN;
SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws_one', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws_one', :'other_id', 'owner');
UPDATE memberships SET role = 'viewer' WHERE user_id = :'owner_id';
COMMIT;
BEGIN;
SELECT set_config('app.user_id', :'other_id', true), set_config('app.workspace_id', :'ws_one', true) \gset
DO $$ BEGIN
    DELETE FROM memberships WHERE user_id = app.current_user_id();
    RAISE EXCEPTION 'FAIL W06: new sole owner removed';
EXCEPTION WHEN integrity_constraint_violation THEN RAISE NOTICE 'PASS W06 ownership hand-over keeps an owner';
END $$;
ROLLBACK;

-- Deleting the whole workspace is not blocked by the last-owner rule.
BEGIN;
SELECT set_config('app.user_id', :'owner_id', true), set_config('app.workspace_id', :'ws_two', true) \gset
DO $$ DECLARE n int; BEGIN
    DELETE FROM workspaces WHERE id = app.current_workspace_id(); GET DIAGNOSTICS n = ROW_COUNT;
    IF n <> 1 OR EXISTS (SELECT 1 FROM app.my_workspaces() WHERE id = current_setting('app.workspace_id')::uuid) THEN
        RAISE EXCEPTION 'FAIL W07: workspace deletion';
    END IF;
    RAISE NOTICE 'PASS W07 owner can delete a workspace; memberships cascade';
END $$;
COMMIT;

\echo 'WORKSPACE CHECKS PASSED'
