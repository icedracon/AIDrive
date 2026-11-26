CREATE TABLE agent_grants (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    issued_by uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (length(btrim(name)) BETWEEN 1 AND 120),
    scope text NOT NULL DEFAULT 'workspace:read' CHECK (scope = 'workspace:read'),
    token_hash bytea NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    expires_at timestamptz NOT NULL DEFAULT statement_timestamp() + interval '7 days',
    revoked_at timestamptz
);

CREATE INDEX agent_grants_workspace_idx ON agent_grants (workspace_id, created_at, id);
CREATE INDEX agent_grants_issuer_idx ON agent_grants (issued_by, workspace_id);
ALTER TABLE agent_grants ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON agent_grants FROM PUBLIC, aidrive_app;

CREATE FUNCTION app.lock_grant_owner(p_workspace uuid) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM 1 FROM public.memberships m
    WHERE m.workspace_id = p_workspace AND m.workspace_id = app.current_workspace_id()
      AND m.user_id = app.current_user_id() AND m.role = 'owner'
    FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'not found' USING ERRCODE = 'insufficient_privilege';
    END IF;
END
$$;
REVOKE ALL ON FUNCTION app.lock_grant_owner(uuid) FROM PUBLIC;

CREATE FUNCTION app.issue_agent_grant(p_workspace uuid, p_id uuid, p_name text, p_hash bytea)
RETURNS void LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM app.lock_grant_owner(p_workspace);
    INSERT INTO public.agent_grants (id, workspace_id, issued_by, name, token_hash)
    VALUES (p_id, p_workspace, app.current_user_id(), btrim(p_name), p_hash);
END
$$;

CREATE FUNCTION app.list_agent_grants(p_workspace uuid)
RETURNS TABLE (id uuid, name text, scope text, created_at timestamptz, expires_at timestamptz, status text)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM app.lock_grant_owner(p_workspace);
    RETURN QUERY SELECT g.id, g.name, g.scope, g.created_at, g.expires_at,
        CASE WHEN g.revoked_at IS NOT NULL THEN 'revoked'
             WHEN g.expires_at <= clock_timestamp() THEN 'expired'
             WHEN NOT EXISTS (SELECT 1 FROM public.memberships m
                 WHERE m.workspace_id = g.workspace_id AND m.user_id = g.issued_by AND m.role = 'owner')
                 THEN 'issuer_unavailable'
             ELSE 'active' END
    FROM public.agent_grants g WHERE g.workspace_id = p_workspace
    ORDER BY g.created_at, g.id;
END
$$;

CREATE FUNCTION app.revoke_agent_grant(p_workspace uuid, p_id uuid) RETURNS boolean
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM app.lock_grant_owner(p_workspace);
    UPDATE public.agent_grants SET revoked_at = coalesce(revoked_at, clock_timestamp())
    WHERE workspace_id = p_workspace AND id = p_id;
    RETURN FOUND;
END
$$;

CREATE FUNCTION app.read_agent_workspace(p_workspace uuid, p_hash bytea)
RETURNS TABLE (id uuid, name text, agent_id uuid, scope text)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    issuer uuid;
    grant_id uuid;
    expiry timestamptz;
BEGIN
    SELECT g.issued_by INTO issuer FROM public.agent_grants g
    WHERE g.workspace_id = p_workspace AND g.token_hash = p_hash;
    IF NOT FOUND THEN RETURN; END IF;
    -- Membership before grant is also the order used by revocation/demotion.
    PERFORM 1 FROM public.memberships m
    WHERE m.workspace_id = p_workspace AND m.user_id = issuer AND m.role = 'owner'
    FOR SHARE;
    IF NOT FOUND THEN RETURN; END IF;
    SELECT g.id, g.expires_at INTO grant_id, expiry FROM public.agent_grants g
    WHERE g.workspace_id = p_workspace AND g.token_hash = p_hash
      AND g.revoked_at IS NULL AND g.expires_at > clock_timestamp()
      AND g.scope = 'workspace:read'
    FOR SHARE;
    IF NOT FOUND OR expiry <= clock_timestamp() THEN RETURN; END IF;
    RETURN QUERY SELECT w.id, w.name, grant_id, 'workspace:read'::text
    FROM public.workspaces w WHERE w.id = p_workspace;
END
$$;

CREATE FUNCTION app.revoke_departed_owner_grants() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF OLD.role = 'owner' AND (TG_OP = 'DELETE' OR NEW.role <> 'owner'
        OR NEW.user_id <> OLD.user_id OR NEW.workspace_id <> OLD.workspace_id) THEN
        UPDATE public.agent_grants SET revoked_at = coalesce(revoked_at, clock_timestamp())
        WHERE workspace_id = OLD.workspace_id AND issued_by = OLD.user_id AND revoked_at IS NULL;
    END IF;
    RETURN NULL;
END
$$;

CREATE TRIGGER memberships_revoke_agent_grants
AFTER UPDATE OR DELETE ON memberships
FOR EACH ROW EXECUTE FUNCTION app.revoke_departed_owner_grants();

REVOKE ALL ON FUNCTION app.revoke_departed_owner_grants() FROM PUBLIC;
REVOKE ALL ON FUNCTION app.issue_agent_grant(uuid, uuid, text, bytea) FROM PUBLIC;
REVOKE ALL ON FUNCTION app.list_agent_grants(uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION app.revoke_agent_grant(uuid, uuid) FROM PUBLIC;
REVOKE ALL ON FUNCTION app.read_agent_workspace(uuid, bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.issue_agent_grant(uuid, uuid, text, bytea) TO aidrive_app;
GRANT EXECUTE ON FUNCTION app.list_agent_grants(uuid) TO aidrive_app;
GRANT EXECUTE ON FUNCTION app.revoke_agent_grant(uuid, uuid) TO aidrive_app;
GRANT EXECUTE ON FUNCTION app.read_agent_workspace(uuid, bytea) TO aidrive_app;
