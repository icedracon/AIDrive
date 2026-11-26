-- Messages addressed to one chat and pushed into it. Many chats (Claude Code
-- sessions, Codex threads) share one assistant key, so a key is not enough to
-- say which chat a message is for. A chat is registered by the client's hooks
-- (client + session id); a message may name the chat it came from and the
-- chat it is for, and a reply goes back to the chat its parent came from.
-- Delivery is a claim: a message is pushed into exactly one chat per key, and
-- the claim also marks it read for that key so pulls do not repeat it.
-- Automatic wakes are limited: at most one per chat every 30 seconds, and at
-- most 6 per thread since the owner last wrote in it; past that the message
-- waits for the chat's next turn and the owner is told once.

CREATE TABLE chats (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    agent_id uuid NOT NULL REFERENCES agent_grants(id),
    client text NOT NULL CHECK (client IN ('claude', 'codex')),
    session_id text NOT NULL CHECK (length(session_id) BETWEEN 1 AND 200),
    cwd text CHECK (cwd IS NULL OR length(cwd) <= 1000),
    first_seen timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_seen timestamptz NOT NULL DEFAULT clock_timestamp(),
    last_wake_at timestamptz,
    UNIQUE (workspace_id, id),
    UNIQUE (workspace_id, client, session_id)
);
CREATE INDEX chats_agent_idx ON chats (workspace_id, agent_id, last_seen DESC);

ALTER TABLE messages ADD COLUMN from_chat uuid, ADD COLUMN to_chat uuid,
    ADD FOREIGN KEY (workspace_id, from_chat) REFERENCES chats(workspace_id, id),
    ADD FOREIGN KEY (workspace_id, to_chat) REFERENCES chats(workspace_id, id),
    ADD CHECK (to_chat IS NULL OR recipient <> 'owner');
CREATE INDEX messages_to_chat_idx ON messages (to_chat) WHERE to_chat IS NOT NULL;

CREATE TABLE message_deliveries (
    workspace_id uuid NOT NULL,
    message_id uuid NOT NULL,
    agent_id uuid NOT NULL REFERENCES agent_grants(id),
    chat_id uuid NOT NULL,
    -- true: delivered by waking the chat (or continuing its turn) without the owner.
    wake boolean NOT NULL,
    delivered_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (message_id, agent_id),
    FOREIGN KEY (workspace_id, message_id) REFERENCES messages(workspace_id, id) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id, chat_id) REFERENCES chats(workspace_id, id) ON DELETE CASCADE
);
CREATE INDEX message_deliveries_chat_idx ON message_deliveries (chat_id, delivered_at DESC);

ALTER TABLE chats ENABLE ROW LEVEL SECURITY;
ALTER TABLE message_deliveries ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON chats, message_deliveries FROM PUBLIC, aidrive_app;
GRANT SELECT ON chats, message_deliveries TO aidrive_app;
CREATE POLICY chats_read ON chats FOR SELECT USING (app.has_role(workspace_id, 'viewer'));
CREATE POLICY message_deliveries_read ON message_deliveries FOR SELECT USING (app.has_role(workspace_id, 'viewer'));

-- Hooks register the chat they run in, under the key of the folder's profile.
-- Any key of the space may (a files:read key can receive messages too).
CREATE FUNCTION app.upsert_chat(p_workspace uuid, p_client text, p_session text, p_cwd text) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_me uuid := nullif(current_setting('app.agent_id', true), '')::uuid;
    v_id uuid;
BEGIN
    PERFORM app.lock_file_access(p_workspace, false);
    IF v_me IS NULL THEN RAISE EXCEPTION 'assistant key required' USING ERRCODE = 'insufficient_privilege'; END IF;
    IF p_client NOT IN ('claude', 'codex') OR p_session IS NULL OR p_session !~ '^[A-Za-z0-9._:-]{1,200}$'
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
REVOKE ALL ON FUNCTION app.upsert_chat(uuid, text, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.upsert_chat(uuid, text, text, text) TO aidrive_app;

-- Definer so readers see the key name behind each chat.
CREATE FUNCTION app.workspace_chats(p_workspace uuid, p_limit integer) RETURNS TABLE (
    id uuid, agent_id uuid, agent_name text, client text, session_id text, cwd text,
    first_seen timestamptz, last_seen timestamptz
)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF NOT app.has_role(p_workspace, 'viewer') THEN
        RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found';
    END IF;
    RETURN QUERY
    SELECT c.id, c.agent_id, g.name, c.client, c.session_id, c.cwd, c.first_seen, c.last_seen
    FROM public.chats c JOIN public.agent_grants g ON g.id = c.agent_id
    WHERE c.workspace_id = p_workspace
    ORDER BY c.last_seen DESC, c.id
    LIMIT least(greatest(coalesce(p_limit, 30), 1), 100);
END
$$;
REVOKE ALL ON FUNCTION app.workspace_chats(uuid, integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.workspace_chats(uuid, integer) TO aidrive_app;

-- send_message with chats. `p_from_chat`: the sender's own chat (a chat of
-- the sending key). `p_to_chat`: the one chat the message is for; it must
-- belong to the recipient key (or any key, for "assistants"). A reply with
-- no `p_to_chat` goes to the chat its parent came from, when that chat
-- belongs to the recipient.
CREATE FUNCTION app.send_message(
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
REVOKE ALL ON FUNCTION app.send_message(uuid, uuid, text, text, text, uuid, jsonb, uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.send_message(uuid, uuid, text, text, text, uuid, jsonb, uuid, uuid) TO aidrive_app;

CREATE OR REPLACE FUNCTION app.send_message(
    p_workspace uuid, p_request uuid, p_to text, p_subject text, p_body text, p_reply_to uuid, p_sources jsonb
) RETURNS uuid
LANGUAGE sql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$ SELECT app.send_message(p_workspace, p_request, p_to, p_subject, p_body, p_reply_to, p_sources, NULL::uuid, NULL::uuid) $$;

-- A chat claims the messages that are for it: those addressed to the chat,
-- and those to its key (or to all assistants) that no chat of its key has
-- taken yet and that its key has not read. One claim per key; concurrent
-- chats of one key race on the primary key and exactly one wins. `p_wake`:
-- the claim wakes the chat (or continues its turn) without the owner, so the
-- per-chat rate limit and the per-thread loop guard apply.
CREATE FUNCTION app.claim_chat_messages(p_workspace uuid, p_chat uuid, p_wake boolean) RETURNS jsonb
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
BEGIN
    PERFORM app.lock_file_access(p_workspace, false);
    IF v_me IS NULL THEN RAISE EXCEPTION 'assistant key required' USING ERRCODE = 'insufficient_privilege'; END IF;
    SELECT * INTO v_chat FROM public.chats c WHERE c.workspace_id = p_workspace AND c.id = p_chat FOR UPDATE;
    IF NOT FOUND OR v_chat.agent_id <> v_me THEN RAISE EXCEPTION 'unknown chat' USING ERRCODE = 'AD422'; END IF;
    UPDATE public.chats SET last_seen = clock_timestamp() WHERE id = p_chat;
    IF coalesce(p_wake, false) AND v_chat.last_wake_at > clock_timestamp() - interval '30 seconds' THEN
        RETURN jsonb_build_object('messages', v_out, 'paused_threads', v_paused, 'rate_limited', true);
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
REVOKE ALL ON FUNCTION app.claim_chat_messages(uuid, uuid, boolean) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.claim_chat_messages(uuid, uuid, boolean) TO aidrive_app;
