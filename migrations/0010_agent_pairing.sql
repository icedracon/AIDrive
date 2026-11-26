-- Pairing lets a desktop assistant connect without anyone copying a key.
-- The client generates its own secret and sends only SHA-256(secret); the
-- owner approves a short code in the web app, which creates an ordinary
-- agent grant for that hash. No secret is transmitted to or stored by the
-- server. Device-flow safeguards (RFC 8628 §5.1, §5.4, §6.1): 8-character
-- codes from a 20-consonant alphabet (~34.5 bits), 10-minute expiry, a
-- per-user limit on failed code lookups, and the approval page names the
-- client and asks the owner to confirm the code.

CREATE TABLE agent_pairings (
    id uuid PRIMARY KEY,
    user_code text NOT NULL UNIQUE CHECK (user_code ~ '^[BCDFGHJKLMNPQRSTVWXZ]{8}$'),
    client_name text NOT NULL CHECK (length(btrim(client_name)) BETWEEN 1 AND 80),
    token_hash bytea NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'denied')),
    workspace_id uuid REFERENCES workspaces(id) ON DELETE CASCADE,
    grant_id uuid REFERENCES agent_grants(id),
    decided_by uuid REFERENCES users(id),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    expires_at timestamptz NOT NULL DEFAULT clock_timestamp() + interval '10 minutes',
    CHECK ((status = 'approved') = (grant_id IS NOT NULL AND workspace_id IS NOT NULL))
);
CREATE INDEX agent_pairings_pending_idx ON agent_pairings (expires_at) WHERE status = 'pending';

CREATE TABLE pairing_lookup_failures (
    user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    at timestamptz NOT NULL DEFAULT clock_timestamp()
);
CREATE INDEX pairing_lookup_failures_idx ON pairing_lookup_failures (user_id, at);

ALTER TABLE agent_pairings ENABLE ROW LEVEL SECURITY;
ALTER TABLE pairing_lookup_failures ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON agent_pairings, pairing_lookup_failures FROM PUBLIC, aidrive_app;

CREATE FUNCTION app.normalize_pairing_code(p_code text) RETURNS text
LANGUAGE sql IMMUTABLE
AS $$ SELECT upper(regexp_replace(coalesce(p_code, ''), '[^A-Za-z]', '', 'g')) $$;
REVOKE ALL ON FUNCTION app.normalize_pairing_code(text) FROM PUBLIC;

-- Unauthenticated by design (the client has no credential yet). Bounded by
-- a global cap on live pending requests; repeats with the same ID and hash
-- return the same code.
CREATE FUNCTION app.start_pairing(p_id uuid, p_client text, p_hash bytea) RETURNS text
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_existing public.agent_pairings%ROWTYPE;
    v_code text;
    v_bytes bytea;
    v_alphabet constant text := 'BCDFGHJKLMNPQRSTVWXZ';
    i int;
BEGIN
    SELECT * INTO v_existing FROM public.agent_pairings WHERE id = p_id;
    IF FOUND THEN
        IF v_existing.token_hash <> p_hash OR v_existing.status <> 'pending' OR v_existing.expires_at <= clock_timestamp() THEN
            RAISE EXCEPTION 'pairing request reused' USING ERRCODE = 'AD409';
        END IF;
        RETURN v_existing.user_code;
    END IF;
    DELETE FROM public.agent_pairings WHERE status = 'pending' AND expires_at < clock_timestamp() - interval '1 day';
    IF (SELECT count(*) FROM public.agent_pairings WHERE status = 'pending' AND expires_at > clock_timestamp()) >= 200 THEN
        RAISE EXCEPTION 'too many pending pairings' USING ERRCODE = 'AD429';
    END IF;
    LOOP
        v_code := '';
        v_bytes := uuid_send(gen_random_uuid()) || uuid_send(gen_random_uuid());
        i := 0;
        -- Rejection sampling keeps every letter equally likely (256 = 12*20 + 16).
        WHILE length(v_code) < 8 AND i < 32 LOOP
            IF get_byte(v_bytes, i) < 240 THEN
                v_code := v_code || substr(v_alphabet, get_byte(v_bytes, i) % 20 + 1, 1);
            END IF;
            i := i + 1;
        END LOOP;
        CONTINUE WHEN length(v_code) < 8;
        BEGIN
            INSERT INTO public.agent_pairings (id, user_code, client_name, token_hash)
            VALUES (p_id, v_code, btrim(p_client), p_hash);
            RETURN v_code;
        EXCEPTION WHEN unique_violation THEN
            IF EXISTS (SELECT 1 FROM public.agent_pairings WHERE id = p_id OR token_hash = p_hash) THEN
                RAISE EXCEPTION 'pairing request reused' USING ERRCODE = 'AD409';
            END IF;
        END;
    END LOOP;
END
$$;
REVOKE ALL ON FUNCTION app.start_pairing(uuid, text, bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.start_pairing(uuid, text, bytea) TO aidrive_app;

-- Resolves a code for a signed-in user; unknown or expired codes count as
-- failures, and ten failures in ten minutes block further lookups.
CREATE FUNCTION app.find_pairing(p_code text) RETURNS public.agent_pairings
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_user uuid := app.current_user_id();
    v_row public.agent_pairings%ROWTYPE;
BEGIN
    IF v_user IS NULL THEN
        RAISE EXCEPTION 'authentication required' USING ERRCODE = 'insufficient_privilege';
    END IF;
    IF (SELECT count(*) FROM public.pairing_lookup_failures f
        WHERE f.user_id = v_user AND f.at > clock_timestamp() - interval '10 minutes') >= 10 THEN
        RAISE EXCEPTION 'too many failed codes' USING ERRCODE = 'AD429';
    END IF;
    SELECT * INTO v_row FROM public.agent_pairings p
    WHERE p.user_code = app.normalize_pairing_code(p_code) AND p.expires_at > clock_timestamp();
    IF NOT FOUND THEN
        INSERT INTO public.pairing_lookup_failures (user_id) VALUES (v_user);
        RETURN NULL;
    END IF;
    RETURN v_row;
END
$$;
REVOKE ALL ON FUNCTION app.find_pairing(text) FROM PUBLIC;

CREATE FUNCTION app.lookup_pairing(p_code text)
RETURNS TABLE (client_name text, status text, expires_at timestamptz)
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_row public.agent_pairings%ROWTYPE;
BEGIN
    v_row := app.find_pairing(p_code);
    IF v_row.id IS NULL THEN RETURN; END IF;
    RETURN QUERY SELECT v_row.client_name, v_row.status, v_row.expires_at;
END
$$;
REVOKE ALL ON FUNCTION app.lookup_pairing(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.lookup_pairing(text) TO aidrive_app;

-- Approval issues an ordinary agent grant for the client's hash, so every
-- existing rule (owner-only, expiry, revocation, demotion) applies unchanged.
-- Returns 'not_found', 'denied' or the new grant ID. Unknown codes return
-- rather than raise, so the recorded failed attempt is not rolled back.
CREATE FUNCTION app.decide_pairing(p_code text, p_workspace uuid, p_scope text, p_decision text) RETURNS text
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_row public.agent_pairings%ROWTYPE;
    v_grant uuid := gen_random_uuid();
BEGIN
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL THEN
        RAISE EXCEPTION 'agents cannot approve pairings' USING ERRCODE = 'insufficient_privilege';
    END IF;
    IF p_decision NOT IN ('approved', 'denied') THEN
        RAISE EXCEPTION 'invalid decision' USING ERRCODE = 'check_violation';
    END IF;
    v_row := app.find_pairing(p_code);
    IF v_row.id IS NULL THEN RETURN 'not_found'; END IF;
    PERFORM 1 FROM public.agent_pairings WHERE id = v_row.id FOR UPDATE;
    SELECT * INTO v_row FROM public.agent_pairings WHERE id = v_row.id;
    IF v_row.status <> 'pending' THEN RAISE EXCEPTION 'already decided' USING ERRCODE = 'AD409'; END IF;
    IF p_decision = 'denied' THEN
        UPDATE public.agent_pairings SET status = 'denied', decided_by = app.current_user_id() WHERE id = v_row.id;
        RETURN 'denied';
    END IF;
    PERFORM app.issue_agent_grant(p_workspace, v_grant, v_row.client_name, v_row.token_hash, p_scope);
    UPDATE public.agent_pairings
    SET status = 'approved', workspace_id = p_workspace, grant_id = v_grant, decided_by = app.current_user_id()
    WHERE id = v_row.id;
    RETURN v_grant::text;
END
$$;
REVOKE ALL ON FUNCTION app.decide_pairing(text, uuid, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.decide_pairing(text, uuid, text, text) TO aidrive_app;

-- Polled by the client with its own secret; reveals nothing without it.
CREATE FUNCTION app.pairing_status(p_id uuid, p_hash bytea)
RETURNS TABLE (status text, workspace_id uuid, workspace_name text, scope text, approver_email text, expires_at timestamptz)
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
    SELECT p.status, p.workspace_id, w.name, g.scope, u.email, p.expires_at
    FROM public.agent_pairings p
    LEFT JOIN public.workspaces w ON w.id = p.workspace_id
    LEFT JOIN public.agent_grants g ON g.id = p.grant_id
    LEFT JOIN public.users u ON u.id = p.decided_by
    WHERE p.id = p_id AND p.token_hash = p_hash
$$;
REVOKE ALL ON FUNCTION app.pairing_status(uuid, bytea) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.pairing_status(uuid, bytea) TO aidrive_app;
