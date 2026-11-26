-- No sign-in on a local install (owner request 2026-11-22, ROADMAP A5).
-- With local trust the computer's account is the owner (0014), so opening the
-- web app on this computer starts the owner's web session without a password
-- or a one-time link. The API calls this only with AIDRIVE_LOCAL_TRUST=1, a
-- loopback bind, a loopback browser Host and a same-origin request.
-- Same single-person rule as app.claim_local_device: no users creates the
-- local owner, exactly one is that person, more than one is refused.

CREATE FUNCTION app.open_local_session(p_session_hash bytea)
RETURNS TABLE (user_id uuid, email text)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
#variable_conflict use_column
DECLARE
    v_user uuid;
    v_email text;
    v_users int;
BEGIN
    IF p_session_hash IS NULL OR length(p_session_hash) <> 32 THEN
        RAISE EXCEPTION 'invalid session hash' USING ERRCODE = 'check_violation';
    END IF;
    PERFORM pg_advisory_xact_lock(hashtext('aidrive.claim_local_device'));
    SELECT count(*) INTO v_users FROM public.users;
    IF v_users > 1 THEN
        RAISE EXCEPTION 'more than one account' USING ERRCODE = 'AD409';
    ELSIF v_users = 0 THEN
        INSERT INTO public.users (email) VALUES ('owner@local.invalid') RETURNING id, users.email INTO v_user, v_email;
    ELSE
        SELECT u.id, u.email INTO v_user, v_email FROM public.users u;
    END IF;
    INSERT INTO public.auth_sessions (user_id, token_hash, expires_at)
    VALUES (v_user, p_session_hash, statement_timestamp() + interval '7 days');
    RETURN QUERY SELECT v_user, v_email;
END
$$;
REVOKE ALL ON FUNCTION app.open_local_session(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.open_local_session(bytea) TO aidrive_app;
