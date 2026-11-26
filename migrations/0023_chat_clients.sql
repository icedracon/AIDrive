-- Every chat can register, not only Claude Code and Codex sessions with
-- hooks: an MCP server without hooks (OpenCode, or hooks not installed)
-- registers its own process as the chat, under its client's name, or
-- 'other' when the profile does not name a known client.
-- Every message an assistant writes to one key is for one chat: when that
-- key has several chats seen in the last 24 hours the sender must name one
-- (to_chat; replies already go to the parent's chat), and with exactly one
-- the message goes to it.

ALTER TABLE chats DROP CONSTRAINT chats_client_check;
ALTER TABLE chats ADD CONSTRAINT chats_client_check CHECK (client IN ('claude', 'codex', 'opencode', 'other'));

CREATE OR REPLACE FUNCTION app.upsert_chat(p_workspace uuid, p_client text, p_session text, p_cwd text) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_me uuid := nullif(current_setting('app.agent_id', true), '')::uuid;
    v_id uuid;
BEGIN
    PERFORM app.lock_file_access(p_workspace, false);
    IF v_me IS NULL THEN RAISE EXCEPTION 'assistant key required' USING ERRCODE = 'insufficient_privilege'; END IF;
    IF p_client NOT IN ('claude', 'codex', 'opencode', 'other') OR p_session IS NULL OR p_session !~ '^[A-Za-z0-9._:-]{1,200}$'
       OR (p_cwd IS NOT NULL AND (length(p_cwd) > 1000 OR p_cwd ~ '[[:cntrl:]]')) THEN
        RAISE EXCEPTION 'invalid chat' USING ERRCODE = 'check_violation';
    END IF;
    INSERT INTO public.chats AS c (workspace_id, agent_id, client, session_id, cwd)
    VALUES (p_workspace, v_me, p_client, p_session, nullif(p_cwd, ''))
    ON CONFLICT (workspace_id, client, session_id) DO UPDATE
        SET agent_id = excluded.agent_id, cwd = coalesce(excluded.cwd, c.cwd), last_seen = clock_timestamp()
    RETURNING c.id INTO v_id;
    RETURN v_id;
END
$$;

CREATE OR REPLACE FUNCTION app.send_message(
    p_workspace uuid, p_request uuid, p_to text, p_subject text, p_body text, p_reply_to uuid, p_sources jsonb,
    p_from_chat uuid, p_to_chat uuid
) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_agent uuid;
    v_to text := lower(btrim(coalesce(p_to, '')));
    v_recipient text;
    v_recipient_agent uuid;
    v_matches int;
    v_thread uuid;
    v_to_chat uuid := p_to_chat;
    v_chat_agent uuid;
    v_chats int;
    v_only_chat uuid;
    v_parent public.messages%ROWTYPE;
    v_id uuid := gen_random_uuid();
    v_prior public.messages%ROWTYPE;
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    v_agent := app.current_writing_agent(p_workspace);
    IF p_request IS NULL OR v_to = '' OR jsonb_typeof(coalesce(p_sources, '[]'::jsonb)) <> 'array'
       OR jsonb_array_length(coalesce(p_sources, '[]'::jsonb)) > 10 THEN
        RAISE EXCEPTION 'invalid message' USING ERRCODE = 'check_violation';
    END IF;
    IF v_to IN ('assistants', 'all') THEN
        v_recipient := 'assistants';
    ELSIF v_to IN ('owner', 'person', 'people') THEN
        v_recipient := 'owner';
    ELSE
        v_recipient := 'agent';
        SELECT count(*), min(g.id::text)::uuid INTO v_matches, v_recipient_agent
        FROM public.agent_grants g
        WHERE g.workspace_id = p_workspace AND g.revoked_at IS NULL AND g.expires_at > clock_timestamp()
          AND (g.id::text = v_to OR lower(g.name) = v_to);
        IF v_matches <> 1 THEN
            RAISE EXCEPTION 'unknown or ambiguous recipient' USING ERRCODE = 'AD421';
        END IF;
    END IF;
    IF p_from_chat IS NOT NULL AND (v_agent IS NULL OR NOT EXISTS (
        SELECT 1 FROM public.chats c WHERE c.workspace_id = p_workspace AND c.id = p_from_chat AND c.agent_id = v_agent)) THEN
        RAISE EXCEPTION 'unknown chat' USING ERRCODE = 'AD422';
    END IF;
    IF p_reply_to IS NULL THEN
        v_thread := v_id;
    ELSE
        SELECT * INTO v_parent FROM public.messages m WHERE m.workspace_id = p_workspace AND m.id = p_reply_to;
        IF NOT FOUND THEN RAISE EXCEPTION 'unknown message' USING ERRCODE = 'foreign_key_violation'; END IF;
        v_thread := v_parent.thread_id;
        IF v_to_chat IS NULL AND v_parent.from_chat IS NOT NULL AND v_recipient <> 'owner'
           AND v_parent.from_chat IS DISTINCT FROM p_from_chat THEN
            SELECT c.agent_id INTO v_chat_agent FROM public.chats c WHERE c.id = v_parent.from_chat;
            IF v_recipient = 'assistants' OR v_chat_agent = v_recipient_agent THEN
                v_to_chat := v_parent.from_chat;
            END IF;
        END IF;
    END IF;
    IF p_to_chat IS NOT NULL THEN
        SELECT c.agent_id INTO v_chat_agent FROM public.chats c WHERE c.workspace_id = p_workspace AND c.id = p_to_chat;
        IF NOT FOUND OR v_recipient = 'owner' OR (v_recipient = 'agent' AND v_chat_agent <> v_recipient_agent) THEN
            RAISE EXCEPTION 'unknown chat' USING ERRCODE = 'AD422';
        END IF;
    END IF;
    -- An assistant writing to one key names the chat: with several recent
    -- chats on that key the message would reach whichever claims it first.
    IF v_to_chat IS NULL AND v_recipient = 'agent' AND v_agent IS NOT NULL THEN
        SELECT count(*), min(c.id::text)::uuid INTO v_chats, v_only_chat FROM public.chats c
        WHERE c.workspace_id = p_workspace AND c.agent_id = v_recipient_agent
          AND c.id IS DISTINCT FROM p_from_chat AND c.last_seen > clock_timestamp() - interval '24 hours';
        IF v_chats > 1 THEN
            RAISE EXCEPTION 'several chats of this key; name one with to_chat' USING ERRCODE = 'AD423';
        END IF;
        v_to_chat := v_only_chat;
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('message' || p_workspace::text || app.current_user_id()::text || p_request::text, 0));
    SELECT * INTO v_prior FROM public.messages m
    WHERE m.workspace_id = p_workspace AND m.actor_id = app.current_user_id() AND m.request_id = p_request;
    IF FOUND THEN
        IF v_prior.body IS DISTINCT FROM btrim(p_body) OR v_prior.recipient IS DISTINCT FROM v_recipient
           OR v_prior.recipient_agent IS DISTINCT FROM v_recipient_agent OR v_prior.agent_id IS DISTINCT FROM v_agent
           OR v_prior.to_chat IS DISTINCT FROM v_to_chat THEN
            RAISE EXCEPTION 'request ID reused with different input' USING ERRCODE = 'AD409';
        END IF;
        RETURN v_prior.id;
    END IF;
    INSERT INTO public.messages (id, workspace_id, thread_id, reply_to, actor_id, agent_id, recipient, recipient_agent,
                                 subject, body, request_id, from_chat, to_chat)
    VALUES (v_id, p_workspace, v_thread, p_reply_to, app.current_user_id(), v_agent, v_recipient, v_recipient_agent,
            nullif(btrim(p_subject), ''), btrim(p_body), p_request, p_from_chat, v_to_chat);
    INSERT INTO public.message_sources (workspace_id, message_id, file_id, version_id)
    SELECT p_workspace, v_id, (s->>'file_id')::uuid, (s->>'version_id')::uuid
    FROM jsonb_array_elements(coalesce(p_sources, '[]'::jsonb)) s
    ON CONFLICT DO NOTHING;
    RETURN v_id;
END
$$;
