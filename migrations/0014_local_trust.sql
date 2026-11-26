-- Local trust: on a single-person install the account of the computer is the
-- owner, so a local assistant can connect itself without anyone signing in.
-- The API only exposes these functions when AIDRIVE_LOCAL_TRUST=1 and it is
-- bound to a loopback address; hosted installs never reach them.
-- An owner device key is a secret held in the computer's user profile. Only
-- its SHA-256 is stored. It can create spaces and issue agent grants for its
-- owner, and mint a one-time sign-in link for the web app.

CREATE TABLE owner_devices (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash bytea NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    label text NOT NULL CHECK (length(btrim(label)) BETWEEN 1 AND 80),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    revoked_at timestamptz
);
CREATE INDEX owner_devices_user_idx ON owner_devices (user_id) WHERE revoked_at IS NULL;

CREATE TABLE local_login_codes (
    code_hash bytea PRIMARY KEY CHECK (length(code_hash) = 32),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at timestamptz NOT NULL DEFAULT clock_timestamp() + interval '2 minutes',
    used_at timestamptz
);

ALTER TABLE owner_devices ENABLE ROW LEVEL SECURITY;
ALTER TABLE local_login_codes ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON owner_devices, local_login_codes FROM PUBLIC, aidrive_app;

-- No users: creates the local owner. Exactly one user: that person is the
-- owner of this computer. More than one: not a single-person install, refused.
CREATE FUNCTION app.claim_local_device(p_hash bytea, p_label text)
RETURNS TABLE (user_id uuid, email text, created boolean)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
#variable_conflict use_column
DECLARE
    v_user uuid;
    v_email text;
    v_created boolean := false;
    v_users int;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtext('aidrive.claim_local_device'));
    SELECT count(*) INTO v_users FROM public.users;
    IF v_users > 1 THEN
        RAISE EXCEPTION 'more than one account' USING ERRCODE = 'AD409';
    ELSIF v_users = 0 THEN
        INSERT INTO public.users (email) VALUES ('owner@local.invalid') RETURNING id, users.email INTO v_user, v_email;
        v_created := true;
    ELSE
        SELECT u.id, u.email INTO v_user, v_email FROM public.users u;
    END IF;
    IF (SELECT count(*) FROM public.owner_devices d WHERE d.user_id = v_user AND d.revoked_at IS NULL) >= 20 THEN
        RAISE EXCEPTION 'too many devices' USING ERRCODE = 'AD429';
    END IF;
    INSERT INTO public.owner_devices (user_id, token_hash, label) VALUES (v_user, p_hash, btrim(p_label));
    RETURN QUERY SELECT v_user, v_email, v_created;
END
$$;
REVOKE ALL ON FUNCTION app.claim_local_device(bytea, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.claim_local_device(bytea, text) TO aidrive_app;

CREATE FUNCTION app.resolve_owner_device(p_hash bytea) RETURNS uuid
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
    SELECT d.user_id FROM public.owner_devices d WHERE d.token_hash = p_hash AND d.revoked_at IS NULL
$$;
REVOKE ALL ON FUNCTION app.resolve_owner_device(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.resolve_owner_device(bytea) TO aidrive_app;

CREATE FUNCTION app.issue_local_login(p_device_hash bytea, p_code_hash bytea) RETURNS boolean
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_user uuid := app.resolve_owner_device(p_device_hash);
BEGIN
    IF v_user IS NULL THEN RETURN false; END IF;
    DELETE FROM public.local_login_codes WHERE expires_at < clock_timestamp() - interval '1 day';
    INSERT INTO public.local_login_codes (code_hash, user_id) VALUES (p_code_hash, v_user);
    RETURN true;
END
$$;
REVOKE ALL ON FUNCTION app.issue_local_login(bytea, bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.issue_local_login(bytea, bytea) TO aidrive_app;

-- Single use and short-lived. Starts the web session here because a local
-- owner has no password row, which app.create_session requires.
CREATE FUNCTION app.redeem_local_login(p_code_hash bytea, p_session_hash bytea) RETURNS boolean
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_user uuid;
BEGIN
    UPDATE public.local_login_codes SET used_at = clock_timestamp()
    WHERE code_hash = p_code_hash AND used_at IS NULL AND expires_at > clock_timestamp()
    RETURNING user_id INTO v_user;
    IF v_user IS NULL THEN RETURN false; END IF;
    INSERT INTO public.auth_sessions (user_id, token_hash, expires_at)
    VALUES (v_user, p_session_hash, statement_timestamp() + interval '7 days');
    RETURN true;
END
$$;
REVOKE ALL ON FUNCTION app.redeem_local_login(bytea, bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.redeem_local_login(bytea, bytea) TO aidrive_app;
