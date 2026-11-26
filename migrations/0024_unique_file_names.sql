-- A space holds one file per name (case-insensitive, per folder). Found by the
-- AI test campaign (2026-11-24): create_file accepted a second "plan.md", so
-- two assistants could each work on their own copy. Existing duplicates are
-- left as they are; only new names and renames are checked.
CREATE FUNCTION app.reject_duplicate_file_name() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(
        'file-name:' || NEW.workspace_id::text || ':' || coalesce(NEW.parent_id::text, '') || ':' || lower(NEW.name), 0));
    IF EXISTS (
        SELECT 1 FROM public.files f
        WHERE f.workspace_id = NEW.workspace_id AND f.parent_id IS NOT DISTINCT FROM NEW.parent_id
          AND lower(f.name) = lower(NEW.name) AND f.id <> NEW.id
    ) THEN
        RAISE EXCEPTION 'a file with this name already exists' USING ERRCODE = 'AD413';
    END IF;
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION app.reject_duplicate_file_name() FROM PUBLIC;
CREATE TRIGGER files_unique_name BEFORE INSERT OR UPDATE OF name, parent_id ON files
FOR EACH ROW EXECUTE FUNCTION app.reject_duplicate_file_name();
