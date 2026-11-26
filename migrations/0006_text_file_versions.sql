ALTER TABLE files ADD COLUMN current_version_id uuid;

CREATE TABLE file_versions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL,
    file_id uuid NOT NULL,
    revision bigint NOT NULL CHECK (revision > 0),
    parent_version_id uuid,
    restored_from uuid,
    content text NOT NULL CHECK (octet_length(content) <= 262144),
    content_hash bytea NOT NULL CHECK (length(content_hash) = 32),
    actor_id uuid NOT NULL REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (workspace_id, file_id, id),
    UNIQUE (workspace_id, file_id, revision),
    FOREIGN KEY (workspace_id, file_id) REFERENCES files(workspace_id, id) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id, file_id, parent_version_id) REFERENCES file_versions(workspace_id, file_id, id),
    FOREIGN KEY (workspace_id, file_id, restored_from) REFERENCES file_versions(workspace_id, file_id, id)
);

ALTER TABLE files ADD CONSTRAINT files_current_version_fk
    FOREIGN KEY (workspace_id, id, current_version_id) REFERENCES file_versions(workspace_id, file_id, id);

CREATE TABLE file_actions (
    workspace_id uuid NOT NULL,
    actor_id uuid NOT NULL REFERENCES users(id),
    request_id uuid NOT NULL,
    file_id uuid NOT NULL,
    version_id uuid NOT NULL UNIQUE,
    operation text NOT NULL CHECK (operation IN ('create', 'update', 'restore')),
    fingerprint bytea NOT NULL CHECK (length(fingerprint) = 32),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (workspace_id, actor_id, request_id),
    FOREIGN KEY (workspace_id, file_id, version_id) REFERENCES file_versions(workspace_id, file_id, id) ON DELETE CASCADE
);

ALTER TABLE file_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE file_actions ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON file_versions, file_actions FROM PUBLIC, aidrive_app;
GRANT SELECT ON file_versions, file_actions TO aidrive_app;
CREATE POLICY versions_read ON file_versions FOR SELECT USING (app.has_role(workspace_id, 'viewer'));
CREATE POLICY actions_read ON file_actions FOR SELECT USING (app.has_role(workspace_id, 'viewer'));

CREATE FUNCTION app.protect_versioned_file() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF current_user <> 'aidrive_owner' AND (
        (TG_OP <> 'INSERT' AND OLD.current_version_id IS NOT NULL)
        OR (TG_OP <> 'DELETE' AND NEW.current_version_id IS NOT NULL)
    ) THEN
        RAISE EXCEPTION 'versioned files require conditional writes' USING ERRCODE = 'insufficient_privilege';
    END IF;
    RETURN CASE WHEN TG_OP = 'DELETE' THEN OLD ELSE NEW END;
END
$$;
REVOKE ALL ON FUNCTION app.protect_versioned_file() FROM PUBLIC;
CREATE TRIGGER files_protect_versions BEFORE INSERT OR UPDATE OR DELETE ON files
FOR EACH ROW EXECUTE FUNCTION app.protect_versioned_file();

CREATE FUNCTION app.lock_file_access(p_workspace uuid, p_write boolean) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE member_role text;
BEGIN
    SELECT m.role INTO member_role FROM public.memberships m
    WHERE m.workspace_id = p_workspace AND m.workspace_id = app.current_workspace_id()
      AND m.user_id = app.current_user_id() FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    IF p_write AND member_role NOT IN ('owner', 'editor') THEN
        RAISE EXCEPTION 'read only' USING ERRCODE = 'insufficient_privilege';
    END IF;
END
$$;
REVOKE ALL ON FUNCTION app.lock_file_access(uuid, boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.lock_file_access(uuid, boolean) TO aidrive_app;

CREATE FUNCTION app.write_text_file(
    p_workspace uuid, p_request uuid, p_operation text, p_file uuid,
    p_base uuid, p_name text, p_content text, p_source uuid
) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    fingerprint bytea;
    prior public.file_actions%ROWTYPE;
    target uuid := p_file;
    head uuid;
    revision_number bigint := 1;
    version_id uuid := gen_random_uuid();
    bytes text := p_content;
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    IF p_request IS NULL OR p_operation IS NULL OR p_operation NOT IN ('create', 'update', 'restore') THEN
        RAISE EXCEPTION 'invalid request' USING ERRCODE = 'check_violation';
    END IF;
    IF p_operation = 'create' THEN
        IF p_file IS NOT NULL OR p_base IS NOT NULL OR p_source IS NOT NULL OR p_name IS NULL
           OR p_name <> btrim(p_name) OR length(p_name) NOT BETWEEN 1 AND 255
           OR p_name ~ '[[:cntrl:]/\\]' OR p_name !~* '\.(txt|md)$' THEN
            RAISE EXCEPTION 'invalid name or create parameters' USING ERRCODE = 'check_violation';
        END IF;
    ELSIF p_file IS NULL OR p_base IS NULL OR p_name IS NOT NULL
          OR (p_operation = 'update' AND p_source IS NOT NULL)
          OR (p_operation = 'restore' AND (p_source IS NULL OR p_content IS NOT NULL)) THEN
        RAISE EXCEPTION 'invalid change parameters' USING ERRCODE = 'check_violation';
    END IF;
    IF p_operation <> 'restore' AND (p_content IS NULL OR octet_length(p_content) > 262144) THEN
        RAISE EXCEPTION 'invalid text size' USING ERRCODE = 'check_violation';
    END IF;
    fingerprint := sha256(convert_to(jsonb_build_array(p_operation, p_file, p_base, p_name, p_content, p_source)::text, 'UTF8'));
    -- Serialize identical request IDs before checking their durable result.
    PERFORM pg_advisory_xact_lock(hashtextextended(p_workspace::text || app.current_user_id()::text || p_request::text, 0));
    SELECT * INTO prior FROM public.file_actions a
    WHERE a.workspace_id = p_workspace AND a.actor_id = app.current_user_id() AND a.request_id = p_request;
    IF FOUND THEN
        IF prior.fingerprint <> fingerprint THEN
            RAISE EXCEPTION 'request ID reused with different input' USING ERRCODE = 'AD409';
        END IF;
        RETURN prior.version_id;
    END IF;
    IF p_operation = 'create' THEN
        target := gen_random_uuid();
        INSERT INTO public.files (id, workspace_id, name, object_key, content_type, byte_size, created_by)
        VALUES (target, p_workspace, p_name, 'pg-text/' || target::text, 'text/plain; charset=utf-8', octet_length(bytes), app.current_user_id());
    ELSE
        SELECT f.current_version_id INTO head FROM public.files f
        WHERE f.workspace_id = p_workspace AND f.id = target AND f.current_version_id IS NOT NULL FOR UPDATE;
        IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
        IF head <> p_base THEN RAISE EXCEPTION 'stale version' USING ERRCODE = 'AD412'; END IF;
        SELECT v.revision + 1 INTO revision_number FROM public.file_versions v WHERE v.id = head;
        IF p_operation = 'restore' THEN
            SELECT v.content INTO bytes FROM public.file_versions v
            WHERE v.workspace_id = p_workspace AND v.file_id = target AND v.id = p_source;
            IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
        END IF;
    END IF;
    INSERT INTO public.file_versions (id, workspace_id, file_id, revision, parent_version_id, restored_from, content, content_hash, actor_id)
    VALUES (version_id, p_workspace, target, revision_number, head, p_source, bytes, sha256(convert_to(bytes, 'UTF8')), app.current_user_id());
    UPDATE public.files SET current_version_id = version_id, byte_size = octet_length(bytes)
    WHERE workspace_id = p_workspace AND id = target AND current_version_id IS NOT DISTINCT FROM head;
    IF NOT FOUND THEN RAISE EXCEPTION 'stale version' USING ERRCODE = 'AD412'; END IF;
    INSERT INTO public.file_actions (workspace_id, actor_id, request_id, file_id, version_id, operation, fingerprint)
    VALUES (p_workspace, app.current_user_id(), p_request, target, version_id, p_operation, fingerprint);
    RETURN version_id;
END
$$;
REVOKE ALL ON FUNCTION app.write_text_file(uuid, uuid, text, uuid, uuid, text, text, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.write_text_file(uuid, uuid, text, uuid, uuid, text, text, uuid) TO aidrive_app;
