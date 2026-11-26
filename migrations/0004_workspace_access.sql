-- Cross-workspace listing for the signed-in user, and last-owner protection.

CREATE FUNCTION app.my_workspaces()
RETURNS TABLE (id uuid, name text, role text, created_at timestamptz)
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT w.id, w.name, m.role, w.created_at
    FROM public.memberships m
    JOIN public.workspaces w ON w.id = m.workspace_id
    WHERE m.user_id = app.current_user_id()
    ORDER BY w.created_at, w.id
$$;

REVOKE ALL ON FUNCTION app.my_workspaces() FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.my_workspaces() TO aidrive_app;

-- Runs as owner to see every membership regardless of the caller's RLS scope.
-- Locking the workspace row serialises concurrent demotions, so two owners
-- cannot remove each other in parallel and leave none.
CREATE FUNCTION app.protect_last_owner() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF OLD.role <> 'owner' OR (TG_OP = 'UPDATE' AND NEW.role = 'owner' AND NEW.workspace_id = OLD.workspace_id) THEN
        RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
    END IF;
    -- Workspace deletion cascades here after the parent row is gone.
    PERFORM 1 FROM public.workspaces w WHERE w.id = OLD.workspace_id FOR UPDATE;
    IF NOT FOUND THEN
        RETURN OLD;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM public.memberships m
        WHERE m.workspace_id = OLD.workspace_id
          AND m.role = 'owner'
          AND m.user_id <> OLD.user_id
    ) THEN
        RAISE EXCEPTION 'a workspace must keep at least one owner'
            USING ERRCODE = 'integrity_constraint_violation';
    END IF;
    RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END
$$;

REVOKE ALL ON FUNCTION app.protect_last_owner() FROM PUBLIC;

CREATE TRIGGER memberships_protect_last_owner
BEFORE UPDATE OR DELETE ON memberships
FOR EACH ROW EXECUTE FUNCTION app.protect_last_owner();
