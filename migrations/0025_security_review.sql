-- Pre-release security review (docs/SECURITY-REVIEW-2026-11.md).
-- N1: a chat's session id is listed to every viewer (workspace_chats), and
-- registering the same client and session id under another key moved the
-- chat, and the messages addressed to it, to that key. A chat now moves only
-- to another key of the same person (a folder rebound to a new profile), or
-- away from a revoked or expired key; otherwise registration is refused.
-- N2: the loop guard counted automatic wakes per thread, so assistants that
-- start a new thread each time woke a chat without limit (only 30 seconds
-- apart). A chat is now also woken at most 12 times, across all threads,
-- until a person writes in the space; the rest wait for its next turn and
-- the owner is told once.

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
        WHERE c.agent_id = excluded.agent_id OR EXISTS (
            SELECT 1 FROM public.agent_grants g
            WHERE g.id = c.agent_id
              AND (g.issued_by = app.current_user_id() OR g.revoked_at IS NOT NULL OR g.expires_at <= clock_timestamp()))
    RETURNING c.id INTO v_id;
    IF v_id IS NULL THEN
        RAISE EXCEPTION 'chat belongs to another person''s key' USING ERRCODE = 'AD409';
    END IF;
    RETURN v_id;
END
$$;

CREATE OR REPLACE FUNCTION app.claim_chat_messages(p_workspace uuid, p_chat uuid, p_wake boolean) RETURNS jsonb
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_me uuid := nullif(current_setting('app.agent_id', true), '')::uuid;
    v_chat public.chats%ROWTYPE;
    v_msg public.messages%ROWTYPE;
    v_out jsonb := '[]';
    v_paused jsonb := '[]';
    v_since timestamptz;
    v_wakes int;
    v_owner_since timestamptz;
    v_chat_wakes int;
BEGIN
    PERFORM app.lock_file_access(p_workspace, false);
    IF v_me IS NULL THEN RAISE EXCEPTION 'assistant key required' USING ERRCODE = 'insufficient_privilege'; END IF;
    SELECT * INTO v_chat FROM public.chats c WHERE c.workspace_id = p_workspace AND c.id = p_chat FOR UPDATE;
    IF NOT FOUND OR v_chat.agent_id <> v_me THEN RAISE EXCEPTION 'unknown chat' USING ERRCODE = 'AD422'; END IF;
    UPDATE public.chats SET last_seen = clock_timestamp() WHERE id = p_chat;
    IF coalesce(p_wake, false) AND v_chat.last_wake_at > clock_timestamp() - interval '30 seconds' THEN
        RETURN jsonb_build_object('messages', v_out, 'paused_threads', v_paused, 'rate_limited', true);
    END IF;
    IF coalesce(p_wake, false) THEN
        SELECT max(x.created_at) INTO v_owner_since FROM public.messages x
        WHERE x.workspace_id = p_workspace AND x.agent_id IS NULL AND x.recipient <> 'owner';
        SELECT count(*) INTO v_chat_wakes FROM public.message_deliveries d
        WHERE d.chat_id = p_chat AND d.wake AND d.delivered_at > coalesce(v_owner_since, '-infinity'::timestamptz);
    END IF;
    FOR v_msg IN
        SELECT m.* FROM public.messages m
        WHERE m.workspace_id = p_workspace
          AND m.created_at > clock_timestamp() - interval '24 hours'
          AND m.from_chat IS DISTINCT FROM p_chat
          AND (m.to_chat = p_chat
               OR (m.to_chat IS NULL AND m.agent_id IS DISTINCT FROM v_me
                   AND (m.recipient = 'assistants' OR m.recipient_agent = v_me)
                   AND NOT EXISTS (SELECT 1 FROM public.message_reads r WHERE r.message_id = m.id AND r.reader_agent = v_me)))
          AND NOT EXISTS (SELECT 1 FROM public.message_deliveries d WHERE d.message_id = m.id AND d.agent_id = v_me)
        ORDER BY m.created_at, m.id
        LIMIT 5
    LOOP
        IF coalesce(p_wake, false) AND v_chat_wakes >= 12 THEN
            INSERT INTO public.messages (workspace_id, thread_id, reply_to, actor_id, agent_id, recipient, subject, body, request_id)
            VALUES (p_workspace, v_msg.thread_id, v_msg.id, app.current_user_id(), v_me, 'owner', 'Automatic wakes paused for a chat',
                    'AIDrive stopped waking this chat automatically after 12 automatic wakes, across all threads, without a message from you. '
                    || 'The pending messages wait for the chat''s next turn. Write any message in this space to let the assistants continue.',
                    md5('chat-wake-pause' || p_chat::text || coalesce(v_owner_since::text, ''))::uuid)
            ON CONFLICT DO NOTHING;
            EXIT;
        END IF;
        IF coalesce(p_wake, false) THEN
            SELECT max(x.created_at) INTO v_since FROM public.messages x
            WHERE x.workspace_id = p_workspace AND x.thread_id = v_msg.thread_id AND x.agent_id IS NULL AND x.recipient <> 'owner';
            SELECT count(*) INTO v_wakes FROM public.message_deliveries d
            JOIN public.messages x ON x.id = d.message_id
            WHERE x.workspace_id = p_workspace AND x.thread_id = v_msg.thread_id AND d.wake
              AND d.delivered_at > coalesce(v_since, '-infinity'::timestamptz);
            IF v_wakes >= 6 THEN
                INSERT INTO public.messages (workspace_id, thread_id, reply_to, actor_id, agent_id, recipient, subject, body, request_id)
                VALUES (p_workspace, v_msg.thread_id, v_msg.id, app.current_user_id(), v_me, 'owner', 'Automatic replies paused',
                        'AIDrive stopped waking chats automatically in this thread after 6 automatic wakes without a message from you. '
                        || 'The pending messages wait for each chat''s next turn. Reply in this thread to let the assistants continue.',
                        md5('wake-pause' || v_msg.thread_id::text || coalesce(v_since::text, ''))::uuid)
                ON CONFLICT DO NOTHING;
                IF NOT v_paused ? v_msg.thread_id::text THEN
                    v_paused := v_paused || to_jsonb(v_msg.thread_id::text);
                END IF;
                CONTINUE;
            END IF;
        END IF;
        INSERT INTO public.message_deliveries (workspace_id, message_id, agent_id, chat_id, wake)
        VALUES (p_workspace, v_msg.id, v_me, p_chat, coalesce(p_wake, false))
        ON CONFLICT DO NOTHING;
        IF NOT FOUND THEN CONTINUE; END IF;
        IF coalesce(p_wake, false) THEN v_chat_wakes := v_chat_wakes + 1; END IF;
        INSERT INTO public.message_reads (workspace_id, message_id, reader_actor, reader_agent)
        VALUES (p_workspace, v_msg.id, app.current_user_id(), v_me)
        ON CONFLICT DO NOTHING;
        v_out := v_out || jsonb_build_object(
            'id', v_msg.id, 'thread_id', v_msg.thread_id, 'reply_to', v_msg.reply_to,
            'subject', v_msg.subject, 'body', v_msg.body,
            'created_at', to_char(v_msg.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
            'from_agent_name', (SELECT g.name FROM public.agent_grants g WHERE g.id = v_msg.agent_id),
            'from_email', CASE WHEN v_msg.agent_id IS NULL THEN (SELECT u.email FROM public.users u WHERE u.id = v_msg.actor_id) END,
            'from_chat', v_msg.from_chat,
            'from_client', (SELECT c.client FROM public.chats c WHERE c.id = v_msg.from_chat),
            'from_cwd', (SELECT c.cwd FROM public.chats c WHERE c.id = v_msg.from_chat),
            'to_chat', v_msg.to_chat);
    END LOOP;
    IF coalesce(p_wake, false) AND jsonb_array_length(v_out) > 0 THEN
        UPDATE public.chats SET last_wake_at = clock_timestamp() WHERE id = p_chat;
    END IF;
    RETURN jsonb_build_object('messages', v_out, 'paused_threads', v_paused, 'rate_limited', false);
END
$$;
