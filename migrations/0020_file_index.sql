-- File index: assistants register files they create outside AIDrive (scratch
-- scripts, reports, worktree files) with where they live (host, path, git
-- worktree and branch), why they exist, and how long they are needed
-- (keep_until). Once keep_until passes, the next project brief on that
-- machine checks whether the file still exists and tells the assistant:
-- delete it, keep it, or extend it. AIDrive never touches the file itself;
-- the assistant decides, and the decision is recorded here.
--
-- status: active (tracked), kept (needed indefinitely), removed (deleted on
-- purpose), missing (a check found it gone). One row per host and path:
-- registering the same path again updates and reactivates it.

CREATE TABLE artifacts (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    path text NOT NULL CHECK (length(btrim(path)) BETWEEN 1 AND 1000),
    host text NOT NULL DEFAULT '' CHECK (length(host) <= 200),
    worktree text CHECK (worktree IS NULL OR length(btrim(worktree)) BETWEEN 1 AND 1000),
    branch text CHECK (branch IS NULL OR length(btrim(branch)) BETWEEN 1 AND 200),
    purpose text NOT NULL CHECK (length(btrim(purpose)) BETWEEN 1 AND 1000),
    keep_until timestamptz,
    status text NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'kept', 'removed', 'missing')),
    actor_id uuid NOT NULL REFERENCES users(id),
    agent_id uuid REFERENCES agent_grants(id),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_checked_at timestamptz,
    last_seen_at timestamptz,
    closed_at timestamptz,
    closed_by uuid REFERENCES users(id),
    closed_agent uuid REFERENCES agent_grants(id),
    note text CHECK (note IS NULL OR length(note) <= 1000),
    UNIQUE (workspace_id, host, path),
    CHECK ((status IN ('removed', 'missing')) = (closed_at IS NOT NULL)),
    CHECK (status <> 'kept' OR keep_until IS NULL)
);
CREATE INDEX artifacts_due_idx ON artifacts (workspace_id, keep_until) WHERE status = 'active';

ALTER TABLE artifacts ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON artifacts FROM PUBLIC, aidrive_app;
GRANT SELECT ON artifacts TO aidrive_app;
CREATE POLICY artifacts_read ON artifacts FOR SELECT USING (app.has_role(workspace_id, 'viewer'));

-- Editors/owners and files:write assistants register. A NULL keep_until
-- means "needed until someone decides"; it is never due.
CREATE FUNCTION app.register_artifact(
    p_workspace uuid, p_path text, p_host text, p_worktree text, p_branch text, p_purpose text, p_keep_until timestamptz
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
    INSERT INTO public.artifacts AS a (workspace_id, path, host, worktree, branch, purpose, keep_until, actor_id, agent_id)
    VALUES (p_workspace, btrim(p_path), coalesce(btrim(p_host), ''), nullif(btrim(p_worktree), ''),
            nullif(btrim(p_branch), ''), btrim(p_purpose), p_keep_until, app.current_user_id(), v_agent)
    ON CONFLICT (workspace_id, host, path) DO UPDATE
    SET worktree = EXCLUDED.worktree, branch = EXCLUDED.branch, purpose = EXCLUDED.purpose,
        keep_until = EXCLUDED.keep_until, status = 'active', actor_id = EXCLUDED.actor_id,
        agent_id = EXCLUDED.agent_id, updated_at = clock_timestamp(), closed_at = NULL, closed_by = NULL,
        closed_agent = NULL, note = NULL
    RETURNING a.id INTO v_id;
    RETURN v_id;
END
$$;
REVOKE ALL ON FUNCTION app.register_artifact(uuid, text, text, text, text, text, timestamptz) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.register_artifact(uuid, text, text, text, text, text, timestamptz) TO aidrive_app;

-- The decision on a registered file. p_action: 'keep' (needed indefinitely),
-- 'extend' (new keep_until, required), 'removed' (the file was deleted).
CREATE FUNCTION app.resolve_artifact(
    p_workspace uuid, p_id uuid, p_action text, p_keep_until timestamptz, p_note text
) RETURNS text
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_agent uuid;
    v_status text;
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    v_agent := app.current_writing_agent(p_workspace);
    IF p_action NOT IN ('keep', 'extend', 'removed')
       OR (p_action = 'extend') <> (p_keep_until IS NOT NULL)
       OR (p_action = 'extend' AND (p_keep_until <= clock_timestamp() OR p_keep_until > clock_timestamp() + interval '400 days')) THEN
        RAISE EXCEPTION 'invalid decision' USING ERRCODE = 'check_violation';
    END IF;
    UPDATE public.artifacts a
    SET status = CASE p_action WHEN 'keep' THEN 'kept' WHEN 'extend' THEN 'active' ELSE 'removed' END,
        keep_until = CASE p_action WHEN 'extend' THEN p_keep_until WHEN 'keep' THEN NULL ELSE a.keep_until END,
        closed_at = CASE p_action WHEN 'removed' THEN clock_timestamp() END,
        closed_by = CASE p_action WHEN 'removed' THEN app.current_user_id() END,
        closed_agent = CASE p_action WHEN 'removed' THEN v_agent END,
        note = nullif(btrim(p_note), ''),
        updated_at = clock_timestamp()
    WHERE a.workspace_id = p_workspace AND a.id = p_id
    RETURNING a.status INTO v_status;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    RETURN v_status;
END
$$;
REVOKE ALL ON FUNCTION app.resolve_artifact(uuid, uuid, text, timestamptz, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.resolve_artifact(uuid, uuid, text, timestamptz, text) TO aidrive_app;

-- What a client saw when it looked for due files on its machine:
-- [{"id": ..., "exists": true|false}]. A file found missing is closed as
-- 'missing'; one that still exists stays due until someone decides.
CREATE FUNCTION app.record_artifact_checks(p_workspace uuid, p_checks jsonb) RETURNS integer
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
        closed_at = CASE WHEN c.present THEN a.closed_at ELSE clock_timestamp() END
    FROM (SELECT (x->>'id')::uuid AS id, (x->>'exists')::boolean AS present FROM jsonb_array_elements(p_checks) x) c
    WHERE a.workspace_id = p_workspace AND a.id = c.id AND a.status = 'active' AND c.present IS NOT NULL;
    GET DIAGNOSTICS v_count = ROW_COUNT;
    RETURN v_count;
END
$$;
REVOKE ALL ON FUNCTION app.record_artifact_checks(uuid, jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.record_artifact_checks(uuid, jsonb) TO aidrive_app;

-- Definer so readers see who registered a file. p_status: NULL or 'all' for
-- every row, 'open' for active and kept, or one status. p_due: only active
-- rows past keep_until. p_host narrows to one machine ('' included).
CREATE FUNCTION app.workspace_artifacts(p_workspace uuid, p_status text, p_due boolean, p_host text, p_limit integer)
RETURNS TABLE (
    id uuid, path text, host text, worktree text, branch text, purpose text, keep_until timestamptz,
    status text, due boolean, actor_email text, agent_id uuid, agent_name text, created_at timestamptz,
    updated_at timestamptz, last_checked_at timestamptz, last_seen_at timestamptz, closed_at timestamptz, note text
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
           u.email, a.agent_id, g.name, a.created_at, a.updated_at, a.last_checked_at, a.last_seen_at, a.closed_at, a.note
    FROM public.artifacts a
    LEFT JOIN public.users u ON u.id = a.actor_id
    LEFT JOIN public.agent_grants g ON g.id = a.agent_id
    WHERE a.workspace_id = p_workspace
      AND (p_status IS NULL OR p_status = 'all' OR (p_status = 'open' AND a.status IN ('active', 'kept')) OR a.status = p_status)
      AND (NOT coalesce(p_due, false) OR (a.status = 'active' AND a.keep_until IS NOT NULL AND a.keep_until <= clock_timestamp()))
      AND (p_host IS NULL OR a.host = p_host)
    ORDER BY (a.status = 'active' AND a.keep_until <= clock_timestamp()) DESC NULLS LAST, a.keep_until NULLS LAST, a.path
    LIMIT least(greatest(coalesce(p_limit, 100), 1), 500);
END
$$;
REVOKE ALL ON FUNCTION app.workspace_artifacts(uuid, text, boolean, text, integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.workspace_artifacts(uuid, text, boolean, text, integer) TO aidrive_app;

-- The activity timeline also shows registered and closed files.
CREATE OR REPLACE FUNCTION app.workspace_activity(p_workspace uuid, p_before timestamptz, p_limit int)
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
               CASE WHEN h.auto_accepted THEN 'handoff.auto_accepted'
                    WHEN h.status = 'rejected' THEN 'handoff.rejected' ELSE 'handoff.approved' END,
               h.decided_by, CASE WHEN h.auto_accepted THEN h.agent_id END, left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.decided_at IS NOT NULL
        UNION ALL
        SELECT h.confirmed_at, 'handoff.kept', h.confirmed_by, NULL, left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.confirmed_at IS NOT NULL
        UNION ALL
        SELECT h.undone_at, 'handoff.undone', h.undone_by, NULL, left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.undone_at IS NOT NULL
        UNION ALL
        SELECT k.created_at, 'knowledge.proposed', k.actor_id, k.agent_id, left(k.statement, 200), NULL, NULL, NULL, NULL
        FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND NOT app.is_derived_item(k.decision_note)
          AND NOT EXISTS (SELECT 1 FROM public.knowledge_items o WHERE o.workspace_id = k.workspace_id AND o.superseded_by = k.id
                          AND o.decision_note = 'Approved with edits as ' || k.id::text)
          AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
        UNION ALL
        SELECT k.decided_at,
               CASE WHEN k.auto_accepted THEN 'knowledge.auto_accepted'
                    WHEN k.status = 'rejected' THEN 'knowledge.rejected' ELSE 'knowledge.approved' END,
               k.decided_by, CASE WHEN k.auto_accepted THEN k.agent_id END, left(k.statement, 200), NULL, NULL, NULL, NULL
        FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND (k.valid_from IS NOT NULL OR k.status = 'rejected')
          AND NOT app.is_derived_item(k.decision_note) AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
        UNION ALL
        SELECT k.confirmed_at, 'knowledge.kept', k.confirmed_by, NULL, left(k.statement, 200), NULL, NULL, NULL, NULL
        FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND k.confirmed_at IS NOT NULL AND NOT app.is_derived_item(k.decision_note)
          AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
        UNION ALL
        SELECT k.invalid_at,
               CASE WHEN k.auto_accepted AND k.confirmed_at IS NULL THEN 'knowledge.undone' ELSE 'knowledge.retired' END,
               k.invalidated_by, NULL, left(k.statement, 200), NULL, NULL, NULL, NULL
        FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND k.status = 'retired'
          AND k.invalidation_note IS DISTINCT FROM 'Handoff undone by owner'
          AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
        UNION ALL
        SELECT g.created_at, 'agent.issued', g.issued_by, g.id, g.scope, NULL, NULL, NULL, NULL
        FROM public.agent_grants g WHERE g.workspace_id = p_workspace
        UNION ALL
        SELECT g.revoked_at, 'agent.revoked', NULL, g.id, g.scope, NULL, NULL, NULL, NULL
        FROM public.agent_grants g WHERE g.workspace_id = p_workspace AND g.revoked_at IS NOT NULL
        UNION ALL
        SELECT m.created_at, 'message.sent', m.actor_id, m.agent_id, left(coalesce(m.subject, m.body), 200),
               NULL, NULL, NULL, NULL
        FROM public.messages m WHERE m.workspace_id = p_workspace
        UNION ALL
        SELECT a.created_at, 'artifact.registered', a.actor_id, a.agent_id, left(a.path, 200), NULL, NULL, NULL, NULL
        FROM public.artifacts a WHERE a.workspace_id = p_workspace
        UNION ALL
        SELECT a.closed_at, 'artifact.' || a.status, a.closed_by, a.closed_agent, left(a.path, 200), NULL, NULL, NULL, NULL
        FROM public.artifacts a WHERE a.workspace_id = p_workspace AND a.closed_at IS NOT NULL
    ) e
    LEFT JOIN public.users u ON u.id = e.actor
    LEFT JOIN public.agent_grants g ON g.id = e.agent_id
    WHERE p_before IS NULL OR e.at < p_before
    ORDER BY e.at DESC, e.kind
    LIMIT least(greatest(coalesce(p_limit, 50), 1), 100);
END
$$;
