-- Credentials and sessions (ADR 0002). aidrive_app has no table privileges
-- here; it can only call the narrow SECURITY DEFINER functions below.

CREATE TABLE auth_credentials (
    user_id uuid PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    password_hash text NOT NULL CHECK (password_hash LIKE '$argon2id$%'),
    updated_at timestamptz NOT NULL DEFAULT statement_timestamp()
);

CREATE TABLE auth_sessions (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash bytea NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    created_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    last_seen_at timestamptz NOT NULL DEFAULT statement_timestamp(),
    expires_at timestamptz NOT NULL,
    revoked_at timestamptz
);

CREATE INDEX auth_sessions_user_idx ON auth_sessions (user_id);

ALTER TABLE auth_credentials ENABLE ROW LEVEL SECURITY;
ALTER TABLE auth_sessions ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON auth_credentials, auth_sessions FROM PUBLIC;

CREATE UNIQUE INDEX users_email_normalized_idx ON users (lower(btrim(email)));

CREATE FUNCTION app.register_user(p_email text, p_password_hash text) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    normalized text := lower(btrim(p_email));
    created uuid;
BEGIN
    IF normalized !~ '^[^@\s]+@[^@\s]+$' OR length(normalized) > 254 THEN
        RAISE EXCEPTION 'invalid email' USING ERRCODE = 'check_violation';
    END IF;
    IF p_password_hash IS NULL OR p_password_hash NOT LIKE '$argon2id$%' THEN
        RAISE EXCEPTION 'password hash must be argon2id' USING ERRCODE = 'check_violation';
    END IF;
    INSERT INTO public.users (email) VALUES (normalized) RETURNING id INTO created;
    INSERT INTO public.auth_credentials (user_id, password_hash) VALUES (created, p_password_hash);
    RETURN created;
END
$$;

-- Returns at most one row; callers must spend equal effort when it is empty.
CREATE FUNCTION app.login_lookup(p_email text) RETURNS TABLE (user_id uuid, password_hash text)
LANGUAGE sql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT u.id, c.password_hash
    FROM public.users u
    JOIN public.auth_credentials c ON c.user_id = u.id
    WHERE lower(btrim(u.email)) = lower(btrim(p_email))
$$;

-- Lifetimes live here so every caller gets the same policy: absolute 7 days, idle 24 hours.
CREATE FUNCTION app.create_session(p_user uuid, p_token_hash bytea) RETURNS timestamptz
LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    expiry timestamptz := statement_timestamp() + interval '7 days';
BEGIN
    IF NOT EXISTS (SELECT 1 FROM public.auth_credentials c WHERE c.user_id = p_user) THEN
        RAISE EXCEPTION 'unknown user' USING ERRCODE = 'insufficient_privilege';
    END IF;
    DELETE FROM public.auth_sessions s
    WHERE s.user_id = p_user
      AND (s.revoked_at IS NOT NULL OR s.expires_at <= statement_timestamp()
           OR s.last_seen_at <= statement_timestamp() - interval '24 hours');
    INSERT INTO public.auth_sessions (user_id, token_hash, expires_at) VALUES (p_user, p_token_hash, expiry);
    RETURN expiry;
END
$$;

CREATE FUNCTION app.resolve_session(p_token_hash bytea) RETURNS uuid
LANGUAGE sql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    UPDATE public.auth_sessions s
    SET last_seen_at = statement_timestamp()
    WHERE s.token_hash = p_token_hash
      AND s.revoked_at IS NULL
      AND s.expires_at > statement_timestamp()
      AND s.last_seen_at > statement_timestamp() - interval '24 hours'
    RETURNING s.user_id
$$;

CREATE FUNCTION app.revoke_session(p_token_hash bytea) RETURNS boolean
LANGUAGE sql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
    WITH revoked AS (
        UPDATE public.auth_sessions s SET revoked_at = statement_timestamp()
        WHERE s.token_hash = p_token_hash AND s.revoked_at IS NULL
        RETURNING 1
    )
    SELECT EXISTS (SELECT 1 FROM revoked)
$$;

REVOKE ALL ON FUNCTION app.register_user(text, text) FROM PUBLIC;
REVOKE ALL ON FUNCTION app.login_lookup(text) FROM PUBLIC;
REVOKE ALL ON FUNCTION app.create_session(uuid, bytea) FROM PUBLIC;
REVOKE ALL ON FUNCTION app.resolve_session(bytea) FROM PUBLIC;
REVOKE ALL ON FUNCTION app.revoke_session(bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.register_user(text, text) TO aidrive_app;
GRANT EXECUTE ON FUNCTION app.login_lookup(text) TO aidrive_app;
GRANT EXECUTE ON FUNCTION app.create_session(uuid, bytea) TO aidrive_app;
GRANT EXECUTE ON FUNCTION app.resolve_session(bytea) TO aidrive_app;
GRANT EXECUTE ON FUNCTION app.revoke_session(bytea) TO aidrive_app;
