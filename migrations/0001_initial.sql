-- Applied as aidrive_owner (see db/bootstrap.sql and scripts/db-check.sh).
-- RLS is enabled but not forced: the owner bypasses it so that the
-- SECURITY DEFINER helpers below can read memberships without recursion.
-- aidrive_app never owns tables and has no BYPASSRLS.

REVOKE ALL ON SCHEMA public FROM PUBLIC;
GRANT USAGE ON SCHEMA public TO aidrive_app;

CREATE SCHEMA app;
REVOKE ALL ON SCHEMA app FROM PUBLIC;
GRANT USAGE ON SCHEMA app TO aidrive_app;

CREATE TABLE users (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    email text NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE workspaces (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 120),
    created_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE memberships (
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('owner', 'editor', 'viewer')),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (workspace_id, user_id)
);

CREATE TABLE files (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    parent_id uuid,
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 255),
    object_key text NOT NULL UNIQUE,
    content_type text NOT NULL,
    byte_size bigint NOT NULL CHECK (byte_size >= 0),
    created_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, id),
    FOREIGN KEY (workspace_id, parent_id) REFERENCES files(workspace_id, id) ON DELETE SET NULL (parent_id)
);

CREATE INDEX files_workspace_parent_idx ON files (workspace_id, parent_id, name);

CREATE TABLE memories (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title text NOT NULL CHECK (length(title) BETWEEN 1 AND 160),
    content text NOT NULL CHECK (length(content) BETWEEN 1 AND 8000),
    approved_by uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, id)
);

CREATE TABLE memory_sources (
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    memory_id uuid NOT NULL REFERENCES memories(id) ON DELETE CASCADE,
    file_id uuid NOT NULL,
    excerpt text NOT NULL CHECK (length(excerpt) <= 2000),
    PRIMARY KEY (memory_id, file_id),
    FOREIGN KEY (workspace_id, memory_id) REFERENCES memories(workspace_id, id) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id, file_id) REFERENCES files(workspace_id, id) ON DELETE CASCADE
);

CREATE FUNCTION app.current_user_id() RETURNS uuid
LANGUAGE sql STABLE
AS $$ SELECT nullif(current_setting('app.user_id', true), '')::uuid $$;

CREATE FUNCTION app.current_workspace_id() RETURNS uuid
LANGUAGE sql STABLE
AS $$ SELECT nullif(current_setting('app.workspace_id', true), '')::uuid $$;

-- True only for the session's selected workspace, so a request scoped to one
-- workspace cannot touch another even when the user belongs to both.
CREATE FUNCTION app.has_role(target_workspace uuid, minimum text) RETURNS boolean
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT target_workspace IS NOT NULL
       AND target_workspace = app.current_workspace_id()
       AND EXISTS (
           SELECT 1
           FROM public.memberships m
           WHERE m.workspace_id = target_workspace
             AND m.user_id = app.current_user_id()
             AND CASE m.role WHEN 'owner' THEN 3 WHEN 'editor' THEN 2 WHEN 'viewer' THEN 1 END
                 >= CASE minimum WHEN 'owner' THEN 3 WHEN 'editor' THEN 2 WHEN 'viewer' THEN 1 END
       )
$$;

-- Workspace + first owner membership must be created atomically; granting the
-- app role direct INSERT on either table would allow self-enrolment.
CREATE FUNCTION app.create_workspace(workspace_name text) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    actor uuid := app.current_user_id();
    created uuid;
BEGIN
    IF actor IS NULL OR NOT EXISTS (SELECT 1 FROM public.users u WHERE u.id = actor) THEN
        RAISE EXCEPTION 'authenticated user required' USING ERRCODE = 'insufficient_privilege';
    END IF;
    INSERT INTO public.workspaces (name, created_by) VALUES (workspace_name, actor) RETURNING id INTO created;
    INSERT INTO public.memberships (workspace_id, user_id, role) VALUES (created, actor, 'owner');
    RETURN created;
END
$$;

REVOKE ALL ON FUNCTION app.current_user_id() FROM PUBLIC;
REVOKE ALL ON FUNCTION app.current_workspace_id() FROM PUBLIC;
REVOKE ALL ON FUNCTION app.has_role(uuid, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION app.create_workspace(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.current_user_id() TO aidrive_app;
GRANT EXECUTE ON FUNCTION app.current_workspace_id() TO aidrive_app;
GRANT EXECUTE ON FUNCTION app.has_role(uuid, text) TO aidrive_app;
GRANT EXECUTE ON FUNCTION app.create_workspace(text) TO aidrive_app;

ALTER TABLE users ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspaces ENABLE ROW LEVEL SECURITY;
ALTER TABLE memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE files ENABLE ROW LEVEL SECURITY;
ALTER TABLE memories ENABLE ROW LEVEL SECURITY;
ALTER TABLE memory_sources ENABLE ROW LEVEL SECURITY;

GRANT SELECT, INSERT, UPDATE ON users TO aidrive_app;
GRANT SELECT, UPDATE, DELETE ON workspaces TO aidrive_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON memberships, files, memories, memory_sources TO aidrive_app;

CREATE POLICY users_self_select ON users FOR SELECT USING (id = app.current_user_id());
CREATE POLICY users_self_insert ON users FOR INSERT WITH CHECK (id = app.current_user_id());
CREATE POLICY users_self_update ON users FOR UPDATE
    USING (id = app.current_user_id()) WITH CHECK (id = app.current_user_id());

CREATE POLICY workspaces_select ON workspaces FOR SELECT USING (app.has_role(id, 'viewer'));
CREATE POLICY workspaces_update ON workspaces FOR UPDATE
    USING (app.has_role(id, 'owner')) WITH CHECK (app.has_role(id, 'owner'));
CREATE POLICY workspaces_delete ON workspaces FOR DELETE USING (app.has_role(id, 'owner'));

CREATE POLICY memberships_select ON memberships FOR SELECT USING (app.has_role(workspace_id, 'viewer'));
CREATE POLICY memberships_insert ON memberships FOR INSERT WITH CHECK (app.has_role(workspace_id, 'owner'));
CREATE POLICY memberships_update ON memberships FOR UPDATE
    USING (app.has_role(workspace_id, 'owner')) WITH CHECK (app.has_role(workspace_id, 'owner'));
CREATE POLICY memberships_delete ON memberships FOR DELETE USING (app.has_role(workspace_id, 'owner'));

CREATE POLICY files_select ON files FOR SELECT USING (app.has_role(workspace_id, 'viewer'));
CREATE POLICY files_insert ON files FOR INSERT
    WITH CHECK (app.has_role(workspace_id, 'editor') AND created_by = app.current_user_id());
CREATE POLICY files_update ON files FOR UPDATE
    USING (app.has_role(workspace_id, 'editor')) WITH CHECK (app.has_role(workspace_id, 'editor'));
CREATE POLICY files_delete ON files FOR DELETE USING (app.has_role(workspace_id, 'editor'));

-- Durable project knowledge is owner-approved in the first release (PRODUCT.md).
CREATE POLICY memories_select ON memories FOR SELECT USING (app.has_role(workspace_id, 'viewer'));
CREATE POLICY memories_insert ON memories FOR INSERT
    WITH CHECK (app.has_role(workspace_id, 'owner') AND approved_by = app.current_user_id());
CREATE POLICY memories_update ON memories FOR UPDATE
    USING (app.has_role(workspace_id, 'owner'))
    WITH CHECK (app.has_role(workspace_id, 'owner') AND approved_by = app.current_user_id());
CREATE POLICY memories_delete ON memories FOR DELETE USING (app.has_role(workspace_id, 'owner'));

CREATE POLICY memory_sources_select ON memory_sources FOR SELECT USING (app.has_role(workspace_id, 'viewer'));
CREATE POLICY memory_sources_insert ON memory_sources FOR INSERT WITH CHECK (app.has_role(workspace_id, 'owner'));
CREATE POLICY memory_sources_update ON memory_sources FOR UPDATE
    USING (app.has_role(workspace_id, 'owner')) WITH CHECK (app.has_role(workspace_id, 'owner'));
CREATE POLICY memory_sources_delete ON memory_sources FOR DELETE USING (app.has_role(workspace_id, 'owner'));
