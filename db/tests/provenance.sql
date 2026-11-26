\set ON_ERROR_STOP 1
BEGIN;
SET LOCAL ROLE aidrive_app;
SELECT set_config('app.user_id', '00000000-0000-4000-8000-00000000000d', true);
SELECT set_config('app.workspace_id', :'workspace_id', true);
DO $$ BEGIN
    UPDATE files SET created_by = '00000000-0000-4000-8000-00000000000b' WHERE name = 'brief.md';
    RAISE EXCEPTION 'FAIL creator attribution was rewritten';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS creator attribution immutable';
END $$;
DO $$ BEGIN
    UPDATE files SET created_at = created_at - interval '1 year' WHERE name = 'brief.md';
    RAISE EXCEPTION 'FAIL creation time was rewritten';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS creation time immutable';
END $$;
DO $$ BEGIN
    UPDATE files SET id = gen_random_uuid() WHERE name = 'brief.md';
    RAISE EXCEPTION 'FAIL file identity was rewritten';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS file identity immutable';
END $$;
DO $$ DECLARE n int; BEGIN
    UPDATE files SET name = 'reviewed.md', updated_at = '2000-01-01' WHERE name = 'brief.md';
    GET DIAGNOSTICS n = ROW_COUNT;
    IF n <> 1 OR NOT EXISTS (SELECT 1 FROM files WHERE name = 'reviewed.md'
        AND created_by = '00000000-0000-4000-8000-00000000000a'
        AND updated_at = statement_timestamp()) THEN
        RAISE EXCEPTION 'FAIL valid editor update or server modification time';
    END IF;
    RAISE NOTICE 'PASS editor can rename without rewriting creator, server stamps modification time';
END $$;
DO $$ DECLARE stamped boolean; BEGIN
    INSERT INTO files (workspace_id, name, object_key, content_type, byte_size, created_by, created_at, updated_at)
    VALUES (app.current_workspace_id(), 'stamp.md', 'stamp-test', 'text/markdown', 1, app.current_user_id(), '2000-01-01', '2000-01-01')
    RETURNING created_at = statement_timestamp() AND updated_at = statement_timestamp() INTO stamped;
    IF NOT stamped THEN RAISE EXCEPTION 'FAIL caller supplied creation time was trusted'; END IF;
    RAISE NOTICE 'PASS server stamps file creation time';
END $$;
ROLLBACK;
