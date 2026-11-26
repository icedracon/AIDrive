-- Preserve canonical identity when v4 namespaces a v3 fallback line key.
-- Only an identical full body from the same known writer can match a published event.
CREATE OR REPLACE FUNCTION app.add_session_events(p_workspace uuid, p_session uuid, p_events jsonb, p_complete boolean)
RETURNS integer
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_inserted integer := 0;
    v_agent uuid;
    v_session_agent uuid;
    v_authorized boolean;
    e jsonb;
    v_hash text;
    v_body text;
    v_n integer;
    v_row public.session_events%ROWTYPE;
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    v_agent := app.current_writing_agent(p_workspace);
    SELECT s.recapture_agent_id, s.recapture_authorized INTO v_session_agent, v_authorized FROM public.sessions s
    WHERE s.workspace_id = p_workspace AND s.id = p_session AND s.actor_id = app.current_user_id()
    FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    IF jsonb_typeof(p_events) IS DISTINCT FROM 'array' OR jsonb_array_length(p_events) > 500 THEN
        RAISE EXCEPTION 'invalid events' USING ERRCODE = 'check_violation';
    END IF;
    FOR e IN SELECT value FROM jsonb_array_elements(p_events) LOOP
        IF NOT (e ? 'chunk_index' OR e ? 'chunk_count' OR e ? 'content_sha256') THEN
            -- Legacy writers remain append-only and cannot shorten a repaired event.
            IF length(e->>'text') NOT BETWEEN 1 AND 4000 THEN
                RAISE EXCEPTION 'invalid legacy event' USING ERRCODE = 'check_violation';
            END IF;
            INSERT INTO public.session_events(workspace_id, session_id, event_key, ordinal, role, at, body, capture_agent_id, capture_provenance_known)
            VALUES(p_workspace, p_session, e->>'key', (e->>'ordinal')::int, e->>'role', (e->>'at')::timestamptz, e->>'text', v_agent, true)
            ON CONFLICT (session_id, event_key) DO NOTHING;
            GET DIAGNOSTICS v_n = ROW_COUNT;
            v_inserted := v_inserted + v_n;
            CONTINUE;
        END IF;
        IF (e->>'chunk_index') IS NULL OR (e->>'chunk_count') IS NULL
           OR (e->>'content_sha256') IS NULL OR (e->>'legacy_sha256') IS NULL THEN
            RAISE EXCEPTION 'incomplete chunk metadata' USING ERRCODE = 'check_violation';
        END IF;
        v_hash := e->>'content_sha256';
        IF EXISTS (SELECT 1 FROM public.session_event_chunks c
            WHERE c.session_id = p_session AND c.event_key = e->>'key' AND c.content_sha256 = v_hash
              AND (c.writer_agent_id IS DISTINCT FROM v_agent OR c.ordinal <> (e->>'ordinal')::int
                   OR c.role <> e->>'role' OR c.at IS DISTINCT FROM (e->>'at')::timestamptz
                   OR c.chunk_count <> (e->>'chunk_count')::int OR c.legacy_sha256 <> e->>'legacy_sha256'
                   OR (c.chunk_index = (e->>'chunk_index')::int AND c.body <> e->>'text'))) THEN
            RAISE EXCEPTION 'conflicting capture fragment' USING ERRCODE = 'check_violation';
        END IF;
        INSERT INTO public.session_event_chunks(workspace_id, session_id, event_key, writer_agent_id,
            content_sha256, legacy_sha256, chunk_index, chunk_count, ordinal, role, at, body)
        VALUES(p_workspace, p_session, e->>'key', v_agent, v_hash, e->>'legacy_sha256', (e->>'chunk_index')::int,
            (e->>'chunk_count')::int, (e->>'ordinal')::int, e->>'role', (e->>'at')::timestamptz, e->>'text')
        ON CONFLICT DO NOTHING;
        SELECT count(*) INTO v_n FROM public.session_event_chunks c
        WHERE c.session_id = p_session AND c.event_key = e->>'key' AND c.content_sha256 = v_hash;
        IF v_n <> (e->>'chunk_count')::int THEN CONTINUE; END IF;
        SELECT string_agg(c.body, '' ORDER BY c.chunk_index) INTO v_body FROM public.session_event_chunks c
        WHERE c.session_id = p_session AND c.event_key = e->>'key' AND c.content_sha256 = v_hash;
        IF encode(sha256(convert_to(v_body, 'UTF8')), 'hex') <> v_hash THEN
            RAISE EXCEPTION 'capture digest mismatch' USING ERRCODE = 'check_violation';
        END IF;
        -- Match the old redacted/truncated representation, including clients whose
        -- physical event keys were content-derived. Preserve that physical key.
        SELECT count(*) INTO v_n FROM public.session_events c
        WHERE c.session_id = p_session AND c.workspace_id = p_workspace
          AND (c.event_key = e->>'key' OR c.capture_source_key = e->>'key');
        IF v_n > 1 THEN RAISE EXCEPTION 'ambiguous capture identity' USING ERRCODE = 'check_violation'; END IF;
        SELECT * INTO v_row FROM public.session_events c
        WHERE c.session_id = p_session AND c.workspace_id = p_workspace
          AND (c.event_key = e->>'key' OR c.capture_source_key = e->>'key');
        IF NOT FOUND THEN
            SELECT count(*) INTO v_n FROM public.session_events c
            WHERE c.session_id = p_session AND c.workspace_id = p_workspace AND c.ordinal = (e->>'ordinal')::int AND c.role = e->>'role'
              AND c.at IS NOT DISTINCT FROM (e->>'at')::timestamptz
              AND ((c.capture_version IS NULL AND encode(sha256(convert_to(c.body, 'UTF8')), 'hex') = e->>'legacy_sha256')
                   OR (c.capture_version = 3 AND c.capture_provenance_known
                       AND c.capture_agent_id IS NOT DISTINCT FROM v_agent AND c.body = v_body));
            IF v_n > 1 THEN RAISE EXCEPTION 'ambiguous legacy event' USING ERRCODE = 'check_violation'; END IF;
            SELECT * INTO v_row FROM public.session_events c
            WHERE c.session_id = p_session AND c.workspace_id = p_workspace AND c.ordinal = (e->>'ordinal')::int AND c.role = e->>'role'
              AND c.at IS NOT DISTINCT FROM (e->>'at')::timestamptz
              AND ((c.capture_version IS NULL AND encode(sha256(convert_to(c.body, 'UTF8')), 'hex') = e->>'legacy_sha256')
                   OR (c.capture_version = 3 AND c.capture_provenance_known
                       AND c.capture_agent_id IS NOT DISTINCT FROM v_agent AND c.body = v_body));
        END IF;
        IF v_row.event_key IS NOT NULL THEN
            IF v_row.ordinal <> (e->>'ordinal')::int OR v_row.role <> e->>'role'
               OR v_row.at IS DISTINCT FROM (e->>'at')::timestamptz
               OR (v_row.capture_version IS NULL AND
                   ((CASE WHEN NOT v_row.capture_provenance_known THEN
                         NOT (v_authorized AND v_session_agent IS NOT DISTINCT FROM v_agent)
                     ELSE v_row.capture_agent_id IS DISTINCT FROM v_agent END)
                     OR encode(sha256(convert_to(v_row.body, 'UTF8')), 'hex') <> e->>'legacy_sha256'))
               OR (v_row.capture_version IS NOT NULL AND
                   (v_row.capture_agent_id IS DISTINCT FROM v_agent OR v_row.body <> v_body)) THEN
                RAISE EXCEPTION 'capture event belongs to another writer or differs' USING ERRCODE = 'check_violation';
            END IF;
            UPDATE public.session_events SET body = v_body, capture_version = 3,
                capture_agent_id = v_agent, capture_provenance_known = true, capture_source_key = e->>'key'
            WHERE session_id = p_session AND event_key = v_row.event_key;
        ELSE
            INSERT INTO public.session_events(workspace_id, session_id, event_key, ordinal, role, at, body,
                capture_version, capture_agent_id, capture_provenance_known, capture_source_key)
            VALUES(p_workspace, p_session, e->>'key', (e->>'ordinal')::int, e->>'role',
                (e->>'at')::timestamptz, v_body, 3, v_agent, true, e->>'key');
            v_inserted := v_inserted + 1;
        END IF;
        DELETE FROM public.session_event_chunks c
        WHERE c.session_id = p_session AND c.event_key = e->>'key' AND c.content_sha256 = v_hash;
    END LOOP;
    UPDATE public.sessions s SET
        event_count = (SELECT count(*) FROM public.session_events c WHERE c.session_id = p_session),
        coverage = CASE WHEN EXISTS(SELECT 1 FROM public.session_event_chunks c WHERE c.session_id = p_session)
                        THEN 'partial' WHEN p_complete THEN 'complete' ELSE s.coverage END
    WHERE s.workspace_id = p_workspace AND s.id = p_session;
    RETURN v_inserted;
END
$$;
