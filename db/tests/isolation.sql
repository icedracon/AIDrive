-- Run as aidrive_app on a freshly migrated database (scripts/db-check.sh).
-- Every seed goes through the same grants and policies the API will use.
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;

\set u_a '00000000-0000-4000-8000-00000000000a'
\set u_b '00000000-0000-4000-8000-00000000000b'
\set u_c '00000000-0000-4000-8000-00000000000c'
\set u_d '00000000-0000-4000-8000-00000000000d'

-- Seed: each user registers itself; A and B each create a workspace.
BEGIN; SELECT set_config('app.user_id', :'u_a', true) \gset
INSERT INTO users (id, email) VALUES (:'u_a', 'a@example.test');
SELECT app.create_workspace('A project') AS ws_a \gset
COMMIT;
BEGIN; SELECT set_config('app.user_id', :'u_b', true) \gset
INSERT INTO users (id, email) VALUES (:'u_b', 'b@example.test');
SELECT app.create_workspace('B project') AS ws_b \gset
COMMIT;
BEGIN; SELECT set_config('app.user_id', :'u_c', true) \gset
INSERT INTO users (id, email) VALUES (:'u_c', 'c@example.test'); COMMIT;
BEGIN; SELECT set_config('app.user_id', :'u_d', true) \gset
INSERT INTO users (id, email) VALUES (:'u_d', 'd@example.test'); COMMIT;

BEGIN;
SELECT set_config('app.user_id', :'u_a', true), set_config('app.workspace_id', :'ws_a', true) \gset
INSERT INTO memberships (workspace_id, user_id, role) VALUES (:'ws_a', :'u_c', 'viewer'), (:'ws_a', :'u_d', 'editor');
INSERT INTO files (workspace_id, name, object_key, content_type, byte_size, created_by)
VALUES (:'ws_a', 'brief.md', 'obj-a-1', 'text/markdown', 10, :'u_a');
COMMIT;
BEGIN;
SELECT set_config('app.user_id', :'u_b', true), set_config('app.workspace_id', :'ws_b', true) \gset
INSERT INTO files (workspace_id, name, object_key, content_type, byte_size, created_by)
VALUES (:'ws_b', 'secret.md', 'obj-b-1', 'text/markdown', 10, :'u_b');
COMMIT;

\echo 'seed ok'

-- T01: the application role is neither privileged nor a table owner.
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = current_user AND (rolsuper OR rolbypassrls))
       OR EXISTS (SELECT 1 FROM pg_tables WHERE schemaname = 'public' AND tableowner = current_user) THEN
        RAISE EXCEPTION 'FAIL T01: app role can bypass RLS';
    END IF;
    RAISE NOTICE 'PASS T01 app role is not superuser, BYPASSRLS or table owner';
END $$;

-- T02: no context sees nothing.
DO $$ BEGIN
    IF (SELECT count(*) FROM users) + (SELECT count(*) FROM workspaces) + (SELECT count(*) FROM memberships)
       + (SELECT count(*) FROM files) + (SELECT count(*) FROM memories) <> 0 THEN
        RAISE EXCEPTION 'FAIL T02: rows visible without context';
    END IF;
    RAISE NOTICE 'PASS T02 no context returns no rows';
END $$;

-- T03: workspace creation requires an authenticated, registered user; direct insert is not granted.
DO $$ BEGIN
    PERFORM app.create_workspace('anonymous');
    RAISE EXCEPTION 'FAIL T03: anonymous workspace creation';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS T03 anonymous create_workspace denied';
END $$;
DO $$ BEGIN
    INSERT INTO workspaces (name, created_by) VALUES ('direct', gen_random_uuid());
    RAISE EXCEPTION 'FAIL T03b: direct workspace insert';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS T03b direct workspace insert denied';
END $$;

-- A non-member pointed at another workspace.
BEGIN;
SELECT set_config('app.user_id', :'u_a', true), set_config('app.workspace_id', :'ws_b', true),
       set_config('test.ws_a', :'ws_a', true) \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM files) + (SELECT count(*) FROM workspaces) + (SELECT count(*) FROM memberships) <> 0 THEN
        RAISE EXCEPTION 'FAIL T04: non-member reads foreign workspace';
    END IF;
    RAISE NOTICE 'PASS T04 non-member reads nothing in foreign workspace';
END $$;
DO $$ BEGIN
    INSERT INTO memberships (workspace_id, user_id, role) VALUES (app.current_workspace_id(), app.current_user_id(), 'owner');
    RAISE EXCEPTION 'FAIL T05: self-enrolment as owner';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS T05 self-enrolment into foreign workspace denied';
END $$;
DO $$ BEGIN
    INSERT INTO files (workspace_id, name, object_key, content_type, byte_size, created_by)
    VALUES (app.current_workspace_id(), 'x', 'obj-x', 'text/plain', 1, app.current_user_id());
    RAISE EXCEPTION 'FAIL T06: write into foreign workspace';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS T06 write into foreign workspace denied';
END $$;
DO $$ DECLARE n int; d int; BEGIN
    UPDATE files SET name = 'pwned' WHERE true; GET DIAGNOSTICS n = ROW_COUNT;
    DELETE FROM files WHERE true; GET DIAGNOSTICS d = ROW_COUNT;
    IF n + d <> 0 THEN RAISE EXCEPTION 'FAIL T07: foreign rows modified'; END IF;
    RAISE NOTICE 'PASS T07 foreign update/delete affect 0 rows';
END $$;
ROLLBACK;

-- A member of A whose request is scoped to A cannot reach B even by id.
BEGIN;
SELECT set_config('app.user_id', :'u_a', true), set_config('app.workspace_id', :'ws_a', true),
       set_config('test.ws_b', :'ws_b', true) \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM files) <> 1 OR EXISTS (SELECT 1 FROM files WHERE name = 'secret.md') THEN
        RAISE EXCEPTION 'FAIL T08: owner view of own workspace wrong';
    END IF;
    RAISE NOTICE 'PASS T08 owner sees exactly own workspace files';
END $$;
DO $$ BEGIN
    UPDATE files SET workspace_id = current_setting('test.ws_b')::uuid WHERE name = 'brief.md';
    RAISE EXCEPTION 'FAIL T09: file moved across workspaces';
EXCEPTION WHEN insufficient_privilege OR foreign_key_violation THEN RAISE NOTICE 'PASS T09 moving a file to another workspace denied';
END $$;
DO $$ BEGIN
    INSERT INTO files (workspace_id, name, object_key, content_type, byte_size, created_by)
    VALUES (app.current_workspace_id(), 'spoof', 'obj-spoof', 'text/plain', 1, '00000000-0000-4000-8000-00000000000d');
    RAISE EXCEPTION 'FAIL T10: created_by spoofed';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS T10 spoofed created_by denied';
END $$;
DO $$ DECLARE m uuid; BEGIN
    INSERT INTO memories (workspace_id, title, content, approved_by)
    VALUES (app.current_workspace_id(), 'Audience', 'Approved audience', app.current_user_id()) RETURNING id INTO m;
    INSERT INTO memory_sources (workspace_id, memory_id, file_id, excerpt)
    SELECT app.current_workspace_id(), m, f.id, 'brief' FROM files f WHERE f.name = 'brief.md';
    RAISE NOTICE 'PASS T11 owner approves memory with same-workspace evidence';
END $$;
DO $$ BEGIN
    INSERT INTO memories (workspace_id, title, content, approved_by)
    VALUES (app.current_workspace_id(), 'Forged', 'x', '00000000-0000-4000-8000-00000000000d');
    RAISE EXCEPTION 'FAIL T12: approval attributed to another user';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS T12 approval attributed to another user denied';
END $$;
DO $$ BEGIN
    INSERT INTO memory_sources (workspace_id, memory_id, file_id, excerpt)
    SELECT current_setting('test.ws_b')::uuid, m.id, gen_random_uuid(), 'x' FROM memories m LIMIT 1;
    RAISE EXCEPTION 'FAIL T13: evidence row in foreign workspace';
EXCEPTION WHEN insufficient_privilege OR foreign_key_violation THEN RAISE NOTICE 'PASS T13 cross-workspace evidence denied';
END $$;
COMMIT;

-- Viewer C in A.
BEGIN;
SELECT set_config('app.user_id', :'u_c', true), set_config('app.workspace_id', :'ws_a', true) \gset
DO $$ DECLARE n int; BEGIN
    IF (SELECT count(*) FROM files) <> 1 OR (SELECT count(*) FROM memories) <> 1 THEN
        RAISE EXCEPTION 'FAIL T14: viewer cannot read';
    END IF;
    UPDATE files SET name = 'renamed' WHERE true; GET DIAGNOSTICS n = ROW_COUNT;
    IF n <> 0 THEN RAISE EXCEPTION 'FAIL T14: viewer modified file'; END IF;
    UPDATE memberships SET role = 'owner' WHERE user_id = app.current_user_id(); GET DIAGNOSTICS n = ROW_COUNT;
    IF n <> 0 THEN RAISE EXCEPTION 'FAIL T14: viewer promoted self'; END IF;
    RAISE NOTICE 'PASS T14 viewer reads, cannot modify files or promote self';
END $$;
DO $$ BEGIN
    INSERT INTO files (workspace_id, name, object_key, content_type, byte_size, created_by)
    VALUES (app.current_workspace_id(), 'v', 'obj-v', 'text/plain', 1, app.current_user_id());
    RAISE EXCEPTION 'FAIL T15: viewer wrote file';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS T15 viewer file insert denied';
END $$;
ROLLBACK;

-- Editor D in A.
BEGIN;
SELECT set_config('app.user_id', :'u_d', true), set_config('app.workspace_id', :'ws_a', true) \gset
DO $$ DECLARE n int; BEGIN
    INSERT INTO files (workspace_id, name, object_key, content_type, byte_size, created_by)
    VALUES (app.current_workspace_id(), 'notes.md', 'obj-a-2', 'text/markdown', 3, app.current_user_id());
    UPDATE memberships SET role = 'owner' WHERE user_id = app.current_user_id(); GET DIAGNOSTICS n = ROW_COUNT;
    IF n <> 0 THEN RAISE EXCEPTION 'FAIL T16: editor promoted self'; END IF;
    DELETE FROM memories WHERE true; GET DIAGNOSTICS n = ROW_COUNT;
    IF n <> 0 THEN RAISE EXCEPTION 'FAIL T16: editor deleted approved memory'; END IF;
    RAISE NOTICE 'PASS T16 editor writes files, cannot promote self or delete memories';
END $$;
DO $$ BEGIN
    INSERT INTO memberships (workspace_id, user_id, role)
    VALUES (app.current_workspace_id(), '00000000-0000-4000-8000-00000000000b', 'owner');
    RAISE EXCEPTION 'FAIL T17: editor granted membership';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS T17 editor membership grant denied';
END $$;
DO $$ BEGIN
    INSERT INTO memories (workspace_id, title, content, approved_by)
    VALUES (app.current_workspace_id(), 'Self-approved', 'x', app.current_user_id());
    RAISE EXCEPTION 'FAIL T18: editor approved memory';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS T18 editor memory approval denied';
END $$;
ROLLBACK;

-- Revocation: after the owner removes C, C's next request sees nothing.
BEGIN;
SELECT set_config('app.user_id', :'u_a', true), set_config('app.workspace_id', :'ws_a', true) \gset
DELETE FROM memberships WHERE user_id = :'u_c';
COMMIT;
BEGIN;
SELECT set_config('app.user_id', :'u_c', true), set_config('app.workspace_id', :'ws_a', true) \gset
DO $$ BEGIN
    IF (SELECT count(*) FROM files) + (SELECT count(*) FROM memories) + (SELECT count(*) FROM workspaces) <> 0 THEN
        RAISE EXCEPTION 'FAIL T19: revoked member still reads';
    END IF;
    RAISE NOTICE 'PASS T19 revoked member reads nothing';
END $$;
ROLLBACK;

\echo 'ALL ISOLATION CHECKS PASSED'
