CREATE FUNCTION app.protect_file_provenance() RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        NEW.created_at := statement_timestamp();
    ELSIF NEW.id IS DISTINCT FROM OLD.id
       OR NEW.workspace_id IS DISTINCT FROM OLD.workspace_id
       OR NEW.created_by IS DISTINCT FROM OLD.created_by
       OR NEW.created_at IS DISTINCT FROM OLD.created_at THEN
        RAISE EXCEPTION 'file identity and creation provenance are immutable'
            USING ERRCODE = 'insufficient_privilege';
    END IF;
    NEW.updated_at := statement_timestamp();
    RETURN NEW;
END
$$;

REVOKE ALL ON FUNCTION app.protect_file_provenance() FROM PUBLIC;

CREATE TRIGGER files_protect_provenance
BEFORE INSERT OR UPDATE ON files
FOR EACH ROW EXECUTE FUNCTION app.protect_file_provenance();
