-- Messages between the people and assistants of a space, replacing ad-hoc
-- message files. A message goes to all assistants, to the owner, or to one
-- assistant key; it can reply within a thread and cite exact file versions.
-- Messages are coordination, not knowledge: nothing here is approved project
-- state, and assistants treat a message as a request to weigh, never as an
-- instruction that overrides the owner. Every reader of the space sees every
-- message so the owner can supervise; read receipts are kept per reader
-- (each assistant key, or the person).

CREATE TABLE messages (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    thread_id uuid NOT NULL,
    reply_to uuid,
    actor_id uuid NOT NULL REFERENCES users(id),
    agent_id uuid REFERENCES agent_grants(id),
    recipient text NOT NULL CHECK (recipient IN ('assistants', 'owner', 'agent')),
    recipient_agent uuid REFERENCES agent_grants(id),
    subject text CHECK (subject IS NULL OR length(btrim(subject)) BETWEEN 1 AND 200),
    body text NOT NULL CHECK (length(btrim(body)) BETWEEN 1 AND 8000),
    request_id uuid NOT NULL,
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (workspace_id, id),
    UNIQUE (workspace_id, actor_id, request_id),
    FOREIGN KEY (workspace_id, reply_to) REFERENCES messages(workspace_id, id),
    CHECK ((recipient = 'agent') = (recipient_agent IS NOT NULL))
);
CREATE INDEX messages_recent_idx ON messages (workspace_id, created_at DESC);
CREATE INDEX messages_thread_idx ON messages (workspace_id, thread_id, created_at);

CREATE TABLE message_sources (
    workspace_id uuid NOT NULL,
    message_id uuid NOT NULL,
    file_id uuid NOT NULL,
    version_id uuid NOT NULL,
    PRIMARY KEY (message_id, version_id),
    FOREIGN KEY (workspace_id, message_id) REFERENCES messages(workspace_id, id) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id, file_id, version_id) REFERENCES file_versions(workspace_id, file_id, id) ON DELETE CASCADE
);

CREATE TABLE message_reads (
    workspace_id uuid NOT NULL,
    message_id uuid NOT NULL,
    reader_actor uuid NOT NULL REFERENCES users(id),
    reader_agent uuid REFERENCES agent_grants(id),
    read_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    FOREIGN KEY (workspace_id, message_id) REFERENCES messages(workspace_id, id) ON DELETE CASCADE
);
CREATE UNIQUE INDEX message_reads_agent_idx ON message_reads (message_id, reader_agent) WHERE reader_agent IS NOT NULL;
CREATE UNIQUE INDEX message_reads_person_idx ON message_reads (message_id, reader_actor) WHERE reader_agent IS NULL;

ALTER TABLE messages ENABLE ROW LEVEL SECURITY;
ALTER TABLE message_sources ENABLE ROW LEVEL SECURITY;
ALTER TABLE message_reads ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON messages, message_sources, message_reads FROM PUBLIC, aidrive_app;
GRANT SELECT ON messages, message_sources, message_reads TO aidrive_app;
CREATE POLICY messages_read ON messages FOR SELECT USING (app.has_role(workspace_id, 'viewer'));
CREATE POLICY message_sources_read ON message_sources FOR SELECT USING (app.has_role(workspace_id, 'viewer'));
CREATE POLICY message_reads_read ON message_reads FOR SELECT USING (app.has_role(workspace_id, 'viewer'));

-- Editors/owners and files:write assistants send. `p_to`: 'assistants',
-- 'owner', or the name or id of an active assistant key of this space.
CREATE FUNCTION app.send_message(
    p_workspace uuid, p_request uuid, p_to text, p_subject text, p_body text, p_reply_to uuid, p_sources jsonb
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
    IF p_reply_to IS NULL THEN
        v_thread := v_id;
    ELSE
        SELECT m.thread_id INTO v_thread FROM public.messages m WHERE m.workspace_id = p_workspace AND m.id = p_reply_to;
        IF NOT FOUND THEN RAISE EXCEPTION 'unknown message' USING ERRCODE = 'foreign_key_violation'; END IF;
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('message' || p_workspace::text || app.current_user_id()::text || p_request::text, 0));
    SELECT * INTO v_prior FROM public.messages m
    WHERE m.workspace_id = p_workspace AND m.actor_id = app.current_user_id() AND m.request_id = p_request;
    IF FOUND THEN
        IF v_prior.body IS DISTINCT FROM btrim(p_body) OR v_prior.recipient IS DISTINCT FROM v_recipient
           OR v_prior.recipient_agent IS DISTINCT FROM v_recipient_agent OR v_prior.agent_id IS DISTINCT FROM v_agent THEN
            RAISE EXCEPTION 'request ID reused with different input' USING ERRCODE = 'AD409';
        END IF;
        RETURN v_prior.id;
    END IF;
    INSERT INTO public.messages (id, workspace_id, thread_id, reply_to, actor_id, agent_id, recipient, recipient_agent,
                                 subject, body, request_id)
    VALUES (v_id, p_workspace, v_thread, p_reply_to, app.current_user_id(), v_agent, v_recipient, v_recipient_agent,
            nullif(btrim(p_subject), ''), btrim(p_body), p_request);
    INSERT INTO public.message_sources (workspace_id, message_id, file_id, version_id)
    SELECT p_workspace, v_id, (s->>'file_id')::uuid, (s->>'version_id')::uuid
    FROM jsonb_array_elements(coalesce(p_sources, '[]'::jsonb)) s
    ON CONFLICT DO NOTHING;
    RETURN v_id;
END
$$;
REVOKE ALL ON FUNCTION app.send_message(uuid, uuid, text, text, text, uuid, jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.send_message(uuid, uuid, text, text, text, uuid, jsonb) TO aidrive_app;

-- Any reader of the space (including files:read assistants) records that it
-- read messages; the receipt belongs to the assistant key or the person.
CREATE FUNCTION app.mark_messages_read(p_workspace uuid, p_ids uuid[]) RETURNS integer
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_count integer;
BEGIN
    PERFORM app.lock_file_access(p_workspace, false);
    IF cardinality(p_ids) > 100 THEN RAISE EXCEPTION 'too many ids' USING ERRCODE = 'check_violation'; END IF;
    INSERT INTO public.message_reads (workspace_id, message_id, reader_actor, reader_agent)
    SELECT p_workspace, m.id, app.current_user_id(), nullif(current_setting('app.agent_id', true), '')::uuid
    FROM public.messages m WHERE m.workspace_id = p_workspace AND m.id = ANY (p_ids)
    ON CONFLICT DO NOTHING;
    GET DIAGNOSTICS v_count = ROW_COUNT;
    RETURN v_count;
END
$$;
REVOKE ALL ON FUNCTION app.mark_messages_read(uuid, uuid[]) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.mark_messages_read(uuid, uuid[]) TO aidrive_app;

-- Definer so readers see sender and recipient names, which their own RLS
-- scope hides. `p_for_me`: for an assistant, messages to all assistants or
-- to its key that it did not send; for a person, messages to the owner that
-- the person did not send.
CREATE FUNCTION app.workspace_messages(
    p_workspace uuid, p_id uuid, p_thread uuid, p_for_me boolean, p_unread boolean, p_limit integer
) RETURNS TABLE (
    id uuid, thread_id uuid, reply_to uuid, actor_email text, agent_id uuid, agent_name text,
    recipient text, recipient_agent uuid, recipient_name text, subject text, body text,
    created_at timestamptz, read_by_me boolean
)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_me uuid := nullif(current_setting('app.agent_id', true), '')::uuid;
BEGIN
    IF NOT app.has_role(p_workspace, 'viewer') THEN
        RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found';
    END IF;
    RETURN QUERY
    SELECT m.id, m.thread_id, m.reply_to, u.email, m.agent_id, g.name, m.recipient, m.recipient_agent, r.name,
           m.subject, m.body, m.created_at,
           EXISTS (SELECT 1 FROM public.message_reads x WHERE x.message_id = m.id
                   AND CASE WHEN v_me IS NOT NULL THEN x.reader_agent = v_me
                            ELSE x.reader_agent IS NULL AND x.reader_actor = app.current_user_id() END)
    FROM public.messages m
    LEFT JOIN public.users u ON u.id = m.actor_id
    LEFT JOIN public.agent_grants g ON g.id = m.agent_id
    LEFT JOIN public.agent_grants r ON r.id = m.recipient_agent
    WHERE m.workspace_id = p_workspace
      AND (p_id IS NULL OR m.id = p_id)
      AND (p_thread IS NULL OR m.thread_id = p_thread)
      AND (NOT coalesce(p_for_me, false) OR CASE
              WHEN v_me IS NOT NULL THEN (m.recipient = 'assistants' OR m.recipient_agent = v_me) AND m.agent_id IS DISTINCT FROM v_me
              ELSE m.recipient = 'owner' AND NOT (m.agent_id IS NULL AND m.actor_id = app.current_user_id()) END)
      AND (NOT coalesce(p_unread, false) OR NOT EXISTS (
              SELECT 1 FROM public.message_reads x WHERE x.message_id = m.id
              AND CASE WHEN v_me IS NOT NULL THEN x.reader_agent = v_me
                       ELSE x.reader_agent IS NULL AND x.reader_actor = app.current_user_id() END))
    ORDER BY m.created_at DESC, m.id
    LIMIT least(greatest(coalesce(p_limit, 50), 1), 100);
END
$$;
REVOKE ALL ON FUNCTION app.workspace_messages(uuid, uuid, uuid, boolean, boolean, integer) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.workspace_messages(uuid, uuid, uuid, boolean, boolean, integer) TO aidrive_app;

-- The activity timeline also shows messages.
CREATE OR REPLACE FUNCTION app.workspace_activity(p_workspace uuid, p_before timestamptz, p_limit int)
RETURNS TABLE (
    at timestamptz,
    kind text,
    actor_email text,
    agent_id uuid,
    agent_name text,
    subject text,
    file_id uuid,
    version_id uuid,
    revision bigint,
    handoff_id uuid
)
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF NOT app.has_role(p_workspace, 'viewer') THEN
        RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found';
    END IF;
    RETURN QUERY
    SELECT e.at, e.kind, u.email, e.agent_id, g.name, e.subject, e.file_id, e.version_id, e.revision, e.handoff_id
    FROM (
        SELECT a.created_at AS at, 'file.' || a.operation AS kind, a.actor_id AS actor, a.agent_id,
               f.name AS subject, a.file_id, a.version_id, v.revision, NULL::uuid AS handoff_id
        FROM public.file_actions a
        JOIN public.file_versions v ON v.id = a.version_id
        JOIN public.files f ON f.workspace_id = a.workspace_id AND f.id = a.file_id
        WHERE a.workspace_id = p_workspace
        UNION ALL
        SELECT h.created_at, 'handoff.proposed', h.actor_id, h.agent_id,
               left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace
        UNION ALL
        SELECT h.decided_at,
               CASE WHEN h.status = 'rejected' THEN 'handoff.rejected' ELSE 'handoff.approved' END,
               h.decided_by, NULL, left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.decided_at IS NOT NULL
        UNION ALL
        SELECT g.created_at, 'agent.issued', g.issued_by, g.id, g.scope, NULL, NULL, NULL, NULL
        FROM public.agent_grants g WHERE g.workspace_id = p_workspace
        UNION ALL
        SELECT g.revoked_at, 'agent.revoked', NULL, g.id, g.scope, NULL, NULL, NULL, NULL
        FROM public.agent_grants g WHERE g.workspace_id = p_workspace AND g.revoked_at IS NOT NULL
        UNION ALL
        SELECT m.created_at, 'message.sent', m.actor_id, m.agent_id, left(coalesce(m.subject, m.body), 200),
               NULL, NULL, NULL, NULL
        FROM public.messages m WHERE m.workspace_id = p_workspace
    ) e
    LEFT JOIN public.users u ON u.id = e.actor
    LEFT JOIN public.agent_grants g ON g.id = e.agent_id
    WHERE p_before IS NULL OR e.at < p_before
    ORDER BY e.at DESC, e.kind
    LIMIT least(greatest(coalesce(p_limit, 50), 1), 100);
END
$$;
