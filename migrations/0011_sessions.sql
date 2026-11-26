-- Captured assistant sessions (research/session-memory.md). Raw session
-- evidence is a separate layer from approved knowledge: nothing here becomes
-- an accepted decision without a handoff the owner approves. Events hold
-- what the user and assistant said plus short tool notes; model reasoning
-- and system instructions are excluded by the capturing client, and text is
-- redacted before it leaves the machine. Search indexes are derived columns.

CREATE TABLE sessions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    actor_id uuid NOT NULL REFERENCES users(id),
    agent_id uuid REFERENCES agent_grants(id),
    client text NOT NULL CHECK (client IN ('claude-code', 'codex')),
    external_id text NOT NULL CHECK (length(external_id) BETWEEN 1 AND 200),
    label text NOT NULL CHECK (length(label) BETWEEN 1 AND 200),
    capture_origin text NOT NULL CHECK (capture_origin IN ('import', 'hook')),
    coverage text NOT NULL DEFAULT 'partial' CHECK (coverage IN ('complete', 'partial')),
    started_at timestamptz,
    ended_at timestamptz,
    event_count integer NOT NULL DEFAULT 0,
    captured_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (workspace_id, id),
    UNIQUE (workspace_id, client, external_id)
);
CREATE INDEX sessions_recent_idx ON sessions (workspace_id, ended_at DESC NULLS LAST);

CREATE TABLE session_events (
    workspace_id uuid NOT NULL,
    session_id uuid NOT NULL,
    event_key text NOT NULL CHECK (length(event_key) BETWEEN 1 AND 200),
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    role text NOT NULL CHECK (role IN ('user', 'assistant', 'tool')),
    at timestamptz,
    body text NOT NULL CHECK (length(body) BETWEEN 1 AND 4000),
    search tsvector GENERATED ALWAYS AS (to_tsvector('simple', body)) STORED,
    PRIMARY KEY (session_id, event_key),
    FOREIGN KEY (workspace_id, session_id) REFERENCES sessions(workspace_id, id) ON DELETE CASCADE
);
CREATE INDEX session_events_search_idx ON session_events USING gin (search);
CREATE INDEX session_events_order_idx ON session_events (session_id, ordinal);

ALTER TABLE sessions ENABLE ROW LEVEL SECURITY;
ALTER TABLE session_events ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON sessions, session_events FROM PUBLIC, aidrive_app;
GRANT SELECT ON sessions, session_events TO aidrive_app;
CREATE POLICY sessions_read ON sessions FOR SELECT USING (app.has_role(workspace_id, 'viewer'));
CREATE POLICY session_events_read ON session_events FOR SELECT USING (app.has_role(workspace_id, 'viewer'));

-- Capturing needs write access (editor/owner, or a files:write agent).
-- Re-capturing the same external session updates bounds instead of duplicating.
CREATE FUNCTION app.upsert_session(
    p_workspace uuid, p_client text, p_external text, p_label text, p_origin text,
    p_started timestamptz, p_ended timestamptz
) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_id uuid;
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    INSERT INTO public.sessions (workspace_id, actor_id, agent_id, client, external_id, label, capture_origin, started_at, ended_at)
    VALUES (p_workspace, app.current_user_id(), app.current_writing_agent(p_workspace), p_client, p_external, btrim(p_label), p_origin, p_started, p_ended)
    ON CONFLICT (workspace_id, client, external_id) DO UPDATE
    SET started_at = least(public.sessions.started_at, excluded.started_at),
        ended_at = greatest(public.sessions.ended_at, excluded.ended_at),
        label = excluded.label
    RETURNING id INTO v_id;
    RETURN v_id;
END
$$;
REVOKE ALL ON FUNCTION app.upsert_session(uuid, text, text, text, text, timestamptz, timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.upsert_session(uuid, text, text, text, text, timestamptz, timestamptz) TO aidrive_app;

-- Idempotent by event key; at most 500 events per call. `p_complete` marks
-- that the client sent every event it found in the source transcript.
CREATE FUNCTION app.add_session_events(p_workspace uuid, p_session uuid, p_events jsonb, p_complete boolean)
RETURNS integer
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_inserted integer;
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    IF NOT EXISTS (SELECT 1 FROM public.sessions s WHERE s.workspace_id = p_workspace AND s.id = p_session) THEN
        RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found';
    END IF;
    IF jsonb_typeof(p_events) <> 'array' OR jsonb_array_length(p_events) > 500 THEN
        RAISE EXCEPTION 'invalid events' USING ERRCODE = 'check_violation';
    END IF;
    INSERT INTO public.session_events (workspace_id, session_id, event_key, ordinal, role, at, body)
    SELECT p_workspace, p_session, e->>'key', (e->>'ordinal')::int, e->>'role', (e->>'at')::timestamptz, e->>'text'
    FROM jsonb_array_elements(p_events) e
    ON CONFLICT (session_id, event_key) DO NOTHING;
    GET DIAGNOSTICS v_inserted = ROW_COUNT;
    UPDATE public.sessions s
    SET event_count = (SELECT count(*) FROM public.session_events e WHERE e.session_id = p_session),
        coverage = CASE WHEN p_complete THEN 'complete' ELSE s.coverage END
    WHERE s.id = p_session;
    RETURN v_inserted;
END
$$;
REVOKE ALL ON FUNCTION app.add_session_events(uuid, uuid, jsonb, boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.add_session_events(uuid, uuid, jsonb, boolean) TO aidrive_app;

-- Owner-only deletion; events and their search index rows go with it.
CREATE FUNCTION app.delete_session(p_workspace uuid, p_session uuid) RETURNS boolean
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL OR NOT app.has_role(p_workspace, 'owner') THEN
        RAISE EXCEPTION 'owner required' USING ERRCODE = 'insufficient_privilege';
    END IF;
    DELETE FROM public.sessions WHERE workspace_id = p_workspace AND id = p_session;
    RETURN FOUND;
END
$$;
REVOKE ALL ON FUNCTION app.delete_session(uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.delete_session(uuid, uuid) TO aidrive_app;
