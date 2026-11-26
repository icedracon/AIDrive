-- Activity timeline: a read-only view derived from the immutable records
-- (file actions, handoffs, agent grants), not a second log that could drift.
-- Definer so members see each other's email and agent names, which their
-- own RLS scope hides; access still requires viewer membership of the
-- selected workspace (or an agent context established for it).

CREATE FUNCTION app.workspace_activity(p_workspace uuid, p_before timestamptz, p_limit int)
RETURNS TABLE (
    at timestamptz,
    kind text,
    actor_email text,
    agent_id uuid,
    agent_name text,
    subject text,
    file_id uuid,
    version_id uuid,
    revision bigint,
    handoff_id uuid
)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF NOT app.has_role(p_workspace, 'viewer') THEN
        RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found';
    END IF;
    RETURN QUERY
    SELECT e.at, e.kind, u.email, e.agent_id, g.name, e.subject, e.file_id, e.version_id, e.revision, e.handoff_id
    FROM (
        SELECT a.created_at AS at, 'file.' || a.operation AS kind, a.actor_id AS actor, a.agent_id,
               f.name AS subject, a.file_id, a.version_id, v.revision, NULL::uuid AS handoff_id
        FROM public.file_actions a
        JOIN public.file_versions v ON v.id = a.version_id
        JOIN public.files f ON f.workspace_id = a.workspace_id AND f.id = a.file_id
        WHERE a.workspace_id = p_workspace
        UNION ALL
        SELECT h.created_at, 'handoff.proposed', h.actor_id, h.agent_id,
               left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace
        UNION ALL
        SELECT h.decided_at,
               CASE WHEN h.status = 'rejected' THEN 'handoff.rejected' ELSE 'handoff.approved' END,
               h.decided_by, NULL, left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.decided_at IS NOT NULL
        UNION ALL
        SELECT g.created_at, 'agent.issued', g.issued_by, g.id, g.scope, NULL, NULL, NULL, NULL
        FROM public.agent_grants g WHERE g.workspace_id = p_workspace
        UNION ALL
        SELECT g.revoked_at, 'agent.revoked', NULL, g.id, g.scope, NULL, NULL, NULL, NULL
        FROM public.agent_grants g WHERE g.workspace_id = p_workspace AND g.revoked_at IS NOT NULL
    ) e
    LEFT JOIN public.users u ON u.id = e.actor
    LEFT JOIN public.agent_grants g ON g.id = e.agent_id
    WHERE p_before IS NULL OR e.at < p_before
    ORDER BY e.at DESC, e.kind
    LIMIT least(greatest(coalesce(p_limit, 50), 1), 100);
END
$$;
REVOKE ALL ON FUNCTION app.workspace_activity(uuid, timestamptz, int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.workspace_activity(uuid, timestamptz, int) TO aidrive_app;
