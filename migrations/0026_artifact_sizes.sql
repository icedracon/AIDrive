-- Disk cleanup on top of the file index (0020), proposed by Codex and asked
-- for by the owner (2026-11-26): build folders, packages and large outputs
-- filled the disk (81 GB of Rust build output in worktrees). Registered files
-- now carry what they are (kind), how big they are (size_bytes, measured by
-- the client), and which session created them. Clients index build folders
-- in bound folders automatically ("refresh": a known path only updates its
-- size and last-seen time, so its review date and decisions stay). When the
-- disk runs low or a review date passes, the brief lists what could be freed
-- and asks; nothing is ever deleted automatically.

ALTER TABLE artifacts
    ADD COLUMN kind text NOT NULL DEFAULT 'file' CHECK (kind IN ('file', 'temp', 'build', 'package', 'artifact')),
    ADD COLUMN size_bytes bigint CHECK (size_bytes IS NULL OR size_bytes >= 0),
    ADD COLUMN session_ref text CHECK (session_ref IS NULL OR length(btrim(session_ref)) BETWEEN 1 AND 200),
    ADD COLUMN size_checked_at timestamptz;

DROP FUNCTION app.register_artifact(uuid, text, text, text, text, text, timestamptz);
-- p_refresh: an automatic re-index. For a path that is already active or
-- kept it records only the size, place and last-seen time; a removed or
-- missing path (the folder came back) is registered again as new.
CREATE FUNCTION app.register_artifact(
    p_workspace uuid, p_path text, p_host text, p_worktree text, p_branch text, p_purpose text, p_keep_until timestamptz,
    p_kind text DEFAULT 'file', p_size bigint DEFAULT NULL, p_session text DEFAULT NULL, p_refresh boolean DEFAULT false
) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_agent uuid;
    v_id uuid;
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    v_agent := app.current_writing_agent(p_workspace);
    IF p_keep_until IS NOT NULL AND p_keep_until > clock_timestamp() + interval '400 days' THEN
        RAISE EXCEPTION 'keep_until too far' USING ERRCODE = 'check_violation';
    END IF;
    IF coalesce(p_refresh, false) THEN
        UPDATE public.artifacts a
        SET size_bytes = coalesce(p_size, a.size_bytes),
            size_checked_at = CASE WHEN p_size IS NULL THEN a.size_checked_at ELSE clock_timestamp() END,
            last_seen_at = clock_timestamp(),
            worktree = coalesce(nullif(btrim(p_worktree), ''), a.worktree),
            branch = coalesce(nullif(btrim(p_branch), ''), a.branch)
        WHERE a.workspace_id = p_workspace AND a.host = coalesce(btrim(p_host), '') AND a.path = btrim(p_path)
          AND a.status IN ('active', 'kept')
        RETURNING a.id INTO v_id;
        IF v_id IS NOT NULL THEN RETURN v_id; END IF;
    END IF;
    INSERT INTO public.artifacts AS a (workspace_id, path, host, worktree, branch, purpose, keep_until, actor_id, agent_id,
                                       kind, size_bytes, session_ref, size_checked_at, last_seen_at)
    VALUES (p_workspace, btrim(p_path), coalesce(btrim(p_host), ''), nullif(btrim(p_worktree), ''),
            nullif(btrim(p_branch), ''), btrim(p_purpose), p_keep_until, app.current_user_id(), v_agent,
            coalesce(p_kind, 'file'), p_size, nullif(btrim(p_session), ''),
            CASE WHEN p_size IS NULL THEN NULL ELSE clock_timestamp() END,
            CASE WHEN coalesce(p_refresh, false) THEN clock_timestamp() END)
    ON CONFLICT (workspace_id, host, path) DO UPDATE
    SET worktree = EXCLUDED.worktree, branch = EXCLUDED.branch, purpose = EXCLUDED.purpose,
        keep_until = EXCLUDED.keep_until, status = 'active', actor_id = EXCLUDED.actor_id,
        agent_id = EXCLUDED.agent_id, updated_at = clock_timestamp(), closed_at = NULL, closed_by = NULL,
        closed_agent = NULL, note = NULL, kind = EXCLUDED.kind,
        size_bytes = coalesce(EXCLUDED.size_bytes, a.size_bytes),
        size_checked_at = coalesce(EXCLUDED.size_checked_at, a.size_checked_at),
        session_ref = coalesce(EXCLUDED.session_ref, a.session_ref),
        last_seen_at = coalesce(EXCLUDED.last_seen_at, a.last_seen_at)
    RETURNING a.id INTO v_id;
    RETURN v_id;
END
$$;
REVOKE ALL ON FUNCTION app.register_artifact(uuid, text, text, text, text, text, timestamptz, text, bigint, text, boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.register_artifact(uuid, text, text, text, text, text, timestamptz, text, bigint, text, boolean) TO aidrive_app;

-- Checks may also report the size a client measured: [{"id", "exists", "size_bytes"?}].
CREATE OR REPLACE FUNCTION app.record_artifact_checks(p_workspace uuid, p_checks jsonb) RETURNS integer
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_count integer;
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    PERFORM app.current_writing_agent(p_workspace);
    IF jsonb_typeof(p_checks) <> 'array' OR jsonb_array_length(p_checks) > 100 THEN
        RAISE EXCEPTION 'invalid checks' USING ERRCODE = 'check_violation';
    END IF;
    UPDATE public.artifacts a
    SET last_checked_at = clock_timestamp(),
        last_seen_at = CASE WHEN c.present THEN clock_timestamp() ELSE a.last_seen_at END,
        status = CASE WHEN c.present THEN a.status ELSE 'missing' END,
        closed_at = CASE WHEN c.present THEN a.closed_at ELSE clock_timestamp() END,
        size_bytes = CASE WHEN c.present AND c.size IS NOT NULL THEN c.size ELSE a.size_bytes END,
        size_checked_at = CASE WHEN c.present AND c.size IS NOT NULL THEN clock_timestamp() ELSE a.size_checked_at END
    FROM (SELECT (x->>'id')::uuid AS id, (x->>'exists')::boolean AS present,
                 CASE WHEN jsonb_typeof(x->'size_bytes') = 'number' AND (x->>'size_bytes')::numeric >= 0
                      THEN (x->>'size_bytes')::bigint END AS size
          FROM jsonb_array_elements(p_checks) x) c
    WHERE a.workspace_id = p_workspace AND a.id = c.id AND a.status IN ('active', 'kept') AND c.present IS NOT NULL;
    GET DIAGNOSTICS v_count = ROW_COUNT;
    RETURN v_count;
END
$$;

DROP FUNCTION app.workspace_artifacts(uuid, text, boolean, text, integer);
CREATE FUNCTION app.workspace_artifacts(p_workspace uuid, p_status text, p_due boolean, p_host text, p_limit integer)
RETURNS TABLE (
    id uuid, path text, host text, worktree text, branch text, purpose text, keep_until timestamptz,
    status text, due boolean, actor_email text, agent_id uuid, agent_name text, created_at timestamptz,
    updated_at timestamptz, last_checked_at timestamptz, last_seen_at timestamptz, closed_at timestamptz, note text,
    kind text, size_bytes bigint, session_ref text, size_checked_at timestamptz
)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF NOT app.has_role(p_workspace, 'viewer') THEN
        RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found';
    END IF;
    RETURN QUERY
    SELECT a.id, a.path, a.host, a.worktree, a.branch, a.purpose, a.keep_until, a.status,
           (a.status = 'active' AND a.keep_until IS NOT NULL AND a.keep_until <= clock_timestamp()),
           u.email, a.agent_id, g.name, a.created_at, a.updated_at, a.last_checked_at, a.last_seen_at, a.closed_at, a.note,
           a.kind, a.size_bytes, a.session_ref, a.size_checked_at
    FROM public.artifacts a
    LEFT JOIN public.users u ON u.id = a.actor_id
    LEFT JOIN public.agent_grants g ON g.id = a.agent_id
    WHERE a.workspace_id = p_workspace
      AND (p_status IS NULL OR p_status = 'all' OR (p_status = 'open' AND a.status IN ('active', 'kept')) OR a.status = p_status)
      AND (NOT coalesce(p_due, false) OR (a.status = 'active' AND a.keep_until IS NOT NULL AND a.keep_until <= clock_timestamp()))
      AND (p_host IS NULL OR a.host = p_host)
    ORDER BY (a.status = 'active' AND a.keep_until <= clock_timestamp()) DESC NULLS LAST, a.size_bytes DESC NULLS LAST,
             a.keep_until NULLS LAST, a.path
    LIMIT least(greatest(coalesce(p_limit, 100), 1), 500);
END
$$;
REVOKE ALL ON FUNCTION app.workspace_artifacts(uuid, text, boolean, text, integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.workspace_artifacts(uuid, text, boolean, text, integer) TO aidrive_app;
