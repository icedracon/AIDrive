-- Agent access to files (MVP: external AI clients via MCP).
-- Scope is a level: workspace:read < files:read < files:write. Grants are
-- issued only by owners and checked against the issuer's current owner
-- membership on every use, so an agent never exceeds its issuer's rights.
-- Agent changes are recorded as the issuer (accountable human) plus agent_id.

ALTER TABLE agent_grants DROP CONSTRAINT agent_grants_scope_check;
ALTER TABLE agent_grants ADD CONSTRAINT agent_grants_scope_check
    CHECK (scope IN ('workspace:read', 'files:read', 'files:write'));

CREATE FUNCTION app.scope_rank(p_scope text) RETURNS int
LANGUAGE sql IMMUTABLE
AS $$ SELECT CASE p_scope WHEN 'workspace:read' THEN 1 WHEN 'files:read' THEN 2 WHEN 'files:write' THEN 3 END $$;
REVOKE ALL ON FUNCTION app.scope_rank(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.scope_rank(text) TO aidrive_app;

-- Versions keep their agent attribution: deleting a grant that authored a
-- version is refused rather than silently erasing who made the change.
ALTER TABLE file_versions ADD COLUMN agent_id uuid REFERENCES agent_grants(id);
ALTER TABLE file_actions ADD COLUMN agent_id uuid REFERENCES agent_grants(id);
CREATE INDEX file_versions_agent_idx ON file_versions (agent_id) WHERE agent_id IS NOT NULL;
CREATE INDEX file_actions_agent_idx ON file_actions (agent_id) WHERE agent_id IS NOT NULL;

DROP FUNCTION app.issue_agent_grant(uuid, uuid, text, bytea);
CREATE FUNCTION app.issue_agent_grant(p_workspace uuid, p_id uuid, p_name text, p_hash bytea, p_scope text)
RETURNS void LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM app.lock_grant_owner(p_workspace);
    INSERT INTO public.agent_grants (id, workspace_id, issued_by, name, token_hash, scope)
    VALUES (p_id, p_workspace, app.current_user_id(), btrim(p_name), p_hash, p_scope);
END
$$;
REVOKE ALL ON FUNCTION app.issue_agent_grant(uuid, uuid, text, bytea, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.issue_agent_grant(uuid, uuid, text, bytea, text) TO aidrive_app;

-- Shared validation for every agent request. Lock order (membership, then
-- grant) matches revocation and demotion; expiry is rechecked after waits.
CREATE FUNCTION app.validate_agent_grant(p_workspace uuid, p_hash bytea, p_scope text)
RETURNS TABLE (grant_id uuid, issuer uuid, scope text)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_issuer uuid;
    v_grant uuid;
    v_scope text;
    v_expiry timestamptz;
BEGIN
    SELECT g.issued_by INTO v_issuer FROM public.agent_grants g
    WHERE g.workspace_id = p_workspace AND g.token_hash = p_hash;
    IF NOT FOUND THEN RETURN; END IF;
    PERFORM 1 FROM public.memberships m
    WHERE m.workspace_id = p_workspace AND m.user_id = v_issuer AND m.role = 'owner'
    FOR SHARE;
    IF NOT FOUND THEN RETURN; END IF;
    SELECT g.id, g.scope, g.expires_at INTO v_grant, v_scope, v_expiry FROM public.agent_grants g
    WHERE g.workspace_id = p_workspace AND g.token_hash = p_hash
      AND g.revoked_at IS NULL AND g.expires_at > clock_timestamp()
    FOR SHARE;
    IF NOT FOUND OR v_expiry <= clock_timestamp() THEN RETURN; END IF;
    IF app.scope_rank(v_scope) < app.scope_rank(p_scope) THEN
        RAISE EXCEPTION 'insufficient agent scope' USING ERRCODE = 'AD403';
    END IF;
    RETURN QUERY SELECT v_grant, v_issuer, v_scope;
END
$$;
REVOKE ALL ON FUNCTION app.validate_agent_grant(uuid, bytea, text) FROM PUBLIC;

-- Establishes the RLS/request context for an agent: issuer as accountable
-- user, the workspace, and the grant as agent. Transaction-local only.
CREATE FUNCTION app.begin_agent_request(p_workspace uuid, p_hash bytea, p_scope text)
RETURNS uuid LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_grant uuid;
    v_issuer uuid;
BEGIN
    SELECT v.grant_id, v.issuer INTO v_grant, v_issuer FROM app.validate_agent_grant(p_workspace, p_hash, p_scope) v;
    IF v_grant IS NULL THEN RETURN NULL; END IF;
    PERFORM set_config('app.user_id', v_issuer::text, true),
            set_config('app.workspace_id', p_workspace::text, true),
            set_config('app.agent_id', v_grant::text, true);
    RETURN v_grant;
END
$$;
REVOKE ALL ON FUNCTION app.begin_agent_request(uuid, bytea, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.begin_agent_request(uuid, bytea, text) TO aidrive_app;

-- Existing read endpoint keeps working for every scope level.
CREATE OR REPLACE FUNCTION app.read_agent_workspace(p_workspace uuid, p_hash bytea)
RETURNS TABLE (id uuid, name text, agent_id uuid, scope text)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    RETURN QUERY SELECT w.id, w.name, v.grant_id, v.scope
    FROM app.validate_agent_grant(p_workspace, p_hash, 'workspace:read') v
    JOIN public.workspaces w ON w.id = p_workspace;
END
$$;

-- app.agent_id is honoured only when it names an active files:write grant
-- issued by the current user for this workspace, so a stray setting cannot
-- attribute a human's change to someone else's agent.
CREATE FUNCTION app.current_writing_agent(p_workspace uuid) RETURNS uuid
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_agent uuid := nullif(current_setting('app.agent_id', true), '')::uuid;
BEGIN
    IF v_agent IS NULL THEN RETURN NULL; END IF;
    IF NOT EXISTS (
        SELECT 1 FROM public.agent_grants g
        WHERE g.id = v_agent AND g.workspace_id = p_workspace
          AND g.issued_by = app.current_user_id() AND g.scope = 'files:write'
          AND g.revoked_at IS NULL AND g.expires_at > clock_timestamp()
    ) THEN
        RAISE EXCEPTION 'agent context invalid' USING ERRCODE = 'insufficient_privilege';
    END IF;
    RETURN v_agent;
END
$$;
REVOKE ALL ON FUNCTION app.current_writing_agent(uuid) FROM PUBLIC;

CREATE FUNCTION app.attribute_file_change() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    NEW.agent_id := app.current_writing_agent(NEW.workspace_id);
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION app.attribute_file_change() FROM PUBLIC;

CREATE TRIGGER file_versions_attribute BEFORE INSERT ON file_versions
FOR EACH ROW EXECUTE FUNCTION app.attribute_file_change();
CREATE TRIGGER file_actions_attribute BEFORE INSERT ON file_actions
FOR EACH ROW EXECUTE FUNCTION app.attribute_file_change();
