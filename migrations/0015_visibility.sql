-- Visibility (docs/research/ai-memory-findings.md, plan M4). Numbered 0015:
-- 0014 is taken by the separate local-trust work (branch local-trust).
-- team: everyone who can read the workspace (viewers and up, files:read
-- agents and up). private: only the person who created the record and the
-- assistants acting for that person (an agent's requests run as its issuing
-- owner). Workspace owners do not see other people's private records.
-- Captured sessions are private by default, so existing sessions become
-- private; knowledge is team by default. Only the human creator changes a
-- session's visibility. Knowledge visibility is chosen when proposing and
-- never changes. Private knowledge steers only its creator's assistants, so
-- its creator decides on it, and it never conflicts with or corrects team
-- knowledge.

ALTER TABLE sessions ADD COLUMN visibility text NOT NULL DEFAULT 'private' CHECK (visibility IN ('private', 'team'));
ALTER TABLE knowledge_items ADD COLUMN visibility text NOT NULL DEFAULT 'team' CHECK (visibility IN ('private', 'team'));

CREATE FUNCTION app.can_see(p_workspace uuid, p_visibility text, p_actor uuid) RETURNS boolean
LANGUAGE sql STABLE SET search_path = pg_catalog, pg_temp
AS $$ SELECT app.has_role(p_workspace, 'viewer') AND (p_visibility = 'team' OR p_actor = app.current_user_id()) $$;
REVOKE ALL ON FUNCTION app.can_see(uuid, text, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.can_see(uuid, text, uuid) TO aidrive_app;

DROP POLICY sessions_read ON sessions;
CREATE POLICY sessions_read ON sessions FOR SELECT USING (app.can_see(workspace_id, visibility, actor_id));
DROP POLICY session_events_read ON session_events;
CREATE POLICY session_events_read ON session_events FOR SELECT USING (EXISTS (
    SELECT 1 FROM public.sessions s
    WHERE s.workspace_id = session_events.workspace_id AND s.id = session_events.session_id
      AND app.can_see(s.workspace_id, s.visibility, s.actor_id)));
DROP POLICY knowledge_items_read ON knowledge_items;
CREATE POLICY knowledge_items_read ON knowledge_items FOR SELECT USING (app.can_see(workspace_id, visibility, actor_id));
DROP POLICY knowledge_evidence_read ON knowledge_evidence;
CREATE POLICY knowledge_evidence_read ON knowledge_evidence FOR SELECT USING (EXISTS (
    SELECT 1 FROM public.knowledge_items k
    WHERE k.workspace_id = knowledge_evidence.workspace_id AND k.id = knowledge_evidence.item_id
      AND app.can_see(k.workspace_id, k.visibility, k.actor_id)));

-- A human member acting in this workspace, not an assistant.
CREATE FUNCTION app.require_human_member(p_workspace uuid) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL THEN
        RAISE EXCEPTION 'assistants cannot do this' USING ERRCODE = 'insufficient_privilege';
    END IF;
    PERFORM 1 FROM public.memberships m
    WHERE m.workspace_id = p_workspace AND m.workspace_id = app.current_workspace_id() AND m.user_id = app.current_user_id()
    FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'member required' USING ERRCODE = 'insufficient_privilege'; END IF;
END
$$;
REVOKE ALL ON FUNCTION app.require_human_member(uuid) FROM PUBLIC;

-- Re-capturing merges only into the same person's session; another person's
-- session with the same external id is refused instead of being joined.
CREATE OR REPLACE FUNCTION app.upsert_session(
    p_workspace uuid, p_client text, p_external text, p_label text, p_origin text,
    p_started timestamptz, p_ended timestamptz
) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_id uuid;
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    INSERT INTO public.sessions (workspace_id, actor_id, agent_id, client, external_id, label, capture_origin, started_at, ended_at)
    VALUES (p_workspace, app.current_user_id(), app.current_writing_agent(p_workspace), p_client, p_external, btrim(p_label), p_origin, p_started, p_ended)
    ON CONFLICT (workspace_id, client, external_id) DO UPDATE
    SET started_at = least(public.sessions.started_at, excluded.started_at),
        ended_at = greatest(public.sessions.ended_at, excluded.ended_at),
        label = excluded.label
    WHERE public.sessions.actor_id = excluded.actor_id
    RETURNING id INTO v_id;
    IF v_id IS NULL THEN
        RAISE EXCEPTION 'session belongs to another person' USING ERRCODE = 'AD409';
    END IF;
    RETURN v_id;
END
$$;

-- Only the capturing person (and their assistants) adds events.
CREATE OR REPLACE FUNCTION app.add_session_events(p_workspace uuid, p_session uuid, p_events jsonb, p_complete boolean)
RETURNS integer
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_inserted integer;
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    IF NOT EXISTS (
        SELECT 1 FROM public.sessions s
        WHERE s.workspace_id = p_workspace AND s.id = p_session AND s.actor_id = app.current_user_id()
    ) THEN
        RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found';
    END IF;
    IF jsonb_typeof(p_events) <> 'array' OR jsonb_array_length(p_events) > 500 THEN
        RAISE EXCEPTION 'invalid events' USING ERRCODE = 'check_violation';
    END IF;
    INSERT INTO public.session_events (workspace_id, session_id, event_key, ordinal, role, at, body)
    SELECT p_workspace, p_session, e->>'key', (e->>'ordinal')::int, e->>'role', (e->>'at')::timestamptz, e->>'text'
    FROM jsonb_array_elements(p_events) e
    ON CONFLICT (session_id, event_key) DO NOTHING;
    GET DIAGNOSTICS v_inserted = ROW_COUNT;
    UPDATE public.sessions s
    SET event_count = (SELECT count(*) FROM public.session_events e WHERE e.session_id = p_session),
        coverage = CASE WHEN p_complete THEN 'complete' ELSE s.coverage END
    WHERE s.id = p_session;
    RETURN v_inserted;
END
$$;

-- A workspace owner, or the person who captured the session, deletes it.
CREATE OR REPLACE FUNCTION app.delete_session(p_workspace uuid, p_session uuid) RETURNS boolean
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM app.require_human_member(p_workspace);
    IF NOT app.has_role(p_workspace, 'owner') AND NOT EXISTS (
        SELECT 1 FROM public.sessions s WHERE s.workspace_id = p_workspace AND s.id = p_session AND s.actor_id = app.current_user_id()
    ) THEN
        RAISE EXCEPTION 'owner required' USING ERRCODE = 'insufficient_privilege';
    END IF;
    DELETE FROM public.sessions WHERE workspace_id = p_workspace AND id = p_session;
    RETURN FOUND;
END
$$;

CREATE FUNCTION app.set_session_visibility(p_workspace uuid, p_session uuid, p_visibility text) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM app.require_human_member(p_workspace);
    IF p_visibility IS NULL OR p_visibility NOT IN ('private', 'team') THEN
        RAISE EXCEPTION 'invalid visibility' USING ERRCODE = 'check_violation';
    END IF;
    UPDATE public.sessions SET visibility = p_visibility
    WHERE workspace_id = p_workspace AND id = p_session AND actor_id = app.current_user_id();
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
END
$$;
REVOKE ALL ON FUNCTION app.set_session_visibility(uuid, uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.set_session_visibility(uuid, uuid, text) TO aidrive_app;

-- Evidence may cite only sessions the proposer can see.
CREATE OR REPLACE FUNCTION app.insert_evidence(p_workspace uuid, p_item uuid, p_evidence jsonb) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF jsonb_typeof(coalesce(p_evidence, '[]'::jsonb)) <> 'array' OR jsonb_array_length(coalesce(p_evidence, '[]'::jsonb)) > 10 THEN
        RAISE EXCEPTION 'invalid evidence' USING ERRCODE = 'check_violation';
    END IF;
    IF EXISTS (
        SELECT 1 FROM jsonb_array_elements(coalesce(p_evidence, '[]'::jsonb)) e
        WHERE e ? 'session_id' AND NOT EXISTS (
            SELECT 1 FROM public.sessions s
            WHERE s.id = (e->>'session_id')::uuid AND s.workspace_id = p_workspace
              AND (s.visibility = 'team' OR s.actor_id = app.current_user_id()))
    ) THEN
        RAISE EXCEPTION 'foreign session evidence' USING ERRCODE = 'foreign_key_violation';
    END IF;
    INSERT INTO public.knowledge_evidence (workspace_id, item_id, file_id, version_id, session_id, event_key, handoff_id, quote)
    SELECT p_workspace, p_item, (e->>'file_id')::uuid, (e->>'version_id')::uuid, (e->>'session_id')::uuid,
           e->>'event_key', (e->>'handoff_id')::uuid, nullif(btrim(e->>'quote'), '')
    FROM jsonb_array_elements(coalesce(p_evidence, '[]'::jsonb)) e;
END
$$;

DROP FUNCTION app.propose_item(uuid, uuid, text, text, text, text, uuid, jsonb);

-- Editors/owners and files:write agents propose. A correction must target an
-- approved item of the same visibility (and, for private items, the same person).
CREATE FUNCTION app.propose_item(
    p_workspace uuid, p_request uuid, p_kind text, p_topic text, p_statement text, p_outcome text,
    p_supersedes uuid, p_evidence jsonb, p_visibility text DEFAULT 'team'
) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_id uuid;
    v_prior public.knowledge_items%ROWTYPE;
    v_agent uuid;
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    v_agent := app.current_writing_agent(p_workspace);
    IF p_request IS NULL OR p_visibility IS NULL OR p_visibility NOT IN ('private', 'team') THEN
        RAISE EXCEPTION 'request id and visibility required' USING ERRCODE = 'check_violation';
    END IF;
    IF p_supersedes IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND k.id = p_supersedes AND k.status = 'approved' AND k.visibility = p_visibility
          AND (p_visibility = 'team' OR k.actor_id = app.current_user_id())
    ) THEN
        RAISE EXCEPTION 'only an approved item of the same visibility can be corrected' USING ERRCODE = 'check_violation';
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('knowledge' || p_workspace::text || app.current_user_id()::text || p_request::text, 0));
    SELECT * INTO v_prior FROM public.knowledge_items k
    WHERE k.workspace_id = p_workspace AND k.actor_id = app.current_user_id() AND k.request_id = p_request;
    IF FOUND THEN
        IF v_prior.kind IS DISTINCT FROM p_kind OR v_prior.statement IS DISTINCT FROM btrim(p_statement)
           OR v_prior.topic IS DISTINCT FROM nullif(lower(btrim(p_topic)), '') OR v_prior.agent_id IS DISTINCT FROM v_agent
           OR v_prior.visibility IS DISTINCT FROM p_visibility THEN
            RAISE EXCEPTION 'request ID reused with different input' USING ERRCODE = 'AD409';
        END IF;
        RETURN v_prior.id;
    END IF;
    INSERT INTO public.knowledge_items (workspace_id, kind, topic, statement, outcome, supersedes, actor_id, agent_id, request_id, visibility)
    VALUES (p_workspace, p_kind, nullif(lower(btrim(p_topic)), ''), btrim(p_statement), p_outcome, p_supersedes,
            app.current_user_id(), v_agent, p_request, p_visibility)
    RETURNING id INTO v_id;
    PERFORM app.insert_evidence(p_workspace, v_id, p_evidence);
    RETURN v_id;
END
$$;
REVOKE ALL ON FUNCTION app.propose_item(uuid, uuid, text, text, text, text, uuid, jsonb, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.propose_item(uuid, uuid, text, text, text, text, uuid, jsonb, text) TO aidrive_app;

-- Conflicts stay inside one visibility: team items with team items, private
-- items with the same person's private items.
CREATE OR REPLACE FUNCTION app.item_conflicts(p_workspace uuid, p_kind text, p_topic text, p_supersedes uuid, p_self uuid)
RETURNS uuid[]
LANGUAGE sql STABLE SET search_path = pg_catalog, pg_temp
AS $$
    SELECT coalesce(array_agg(c.id ORDER BY c.created_at, c.id), '{}')
    FROM public.knowledge_items c
    JOIN public.knowledge_items me ON me.workspace_id = p_workspace AND me.id = p_self
    WHERE c.workspace_id = p_workspace AND c.status = 'approved' AND p_kind <> 'attempt'
      AND c.kind = p_kind AND c.topic = p_topic AND c.id <> p_self AND c.id IS DISTINCT FROM p_supersedes
      AND c.visibility = me.visibility AND (me.visibility = 'team' OR c.actor_id = me.actor_id)
$$;

-- Team items: the workspace owner decides. Private items: their creator
-- decides; to anyone else they do not exist.
CREATE FUNCTION app.require_decider(p_workspace uuid, p_visibility text, p_actor uuid) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF p_visibility = 'team' THEN
        PERFORM app.require_human_owner(p_workspace);
    ELSE
        PERFORM app.require_human_member(p_workspace);
        IF p_actor IS DISTINCT FROM app.current_user_id() THEN
            RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found';
        END IF;
    END IF;
END
$$;
REVOKE ALL ON FUNCTION app.require_decider(uuid, text, uuid) FROM PUBLIC;

CREATE OR REPLACE FUNCTION app.decide_item(p_workspace uuid, p_item uuid, p_decision text, p_note text, p_statement text,
                                           p_resolution text DEFAULT NULL)
RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_item public.knowledge_items%ROWTYPE;
    v_result uuid := p_item;
    v_now timestamptz := clock_timestamp();
    v_conflicts uuid[];
BEGIN
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL THEN
        RAISE EXCEPTION 'agents cannot decide' USING ERRCODE = 'insufficient_privilege';
    END IF;
    IF p_decision NOT IN ('approved', 'rejected') OR p_resolution NOT IN ('replace', 'keep_both') THEN
        RAISE EXCEPTION 'invalid decision' USING ERRCODE = 'check_violation';
    END IF;
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    PERFORM app.require_decider(p_workspace, v_item.visibility, v_item.actor_id);
    IF v_item.status <> 'proposed' THEN RAISE EXCEPTION 'already decided' USING ERRCODE = 'AD409'; END IF;
    IF p_decision = 'rejected' THEN
        UPDATE public.knowledge_items SET status = 'rejected', decided_by = app.current_user_id(),
            decided_at = v_now, decision_note = nullif(btrim(p_note), '')
        WHERE id = p_item;
        RETURN p_item;
    END IF;
    PERFORM 1 FROM public.files f JOIN public.knowledge_evidence e ON e.workspace_id = f.workspace_id AND e.file_id = f.id
    WHERE e.item_id = p_item ORDER BY f.id FOR SHARE OF f;
    IF EXISTS (
        SELECT 1 FROM public.knowledge_evidence e JOIN public.files f ON f.workspace_id = e.workspace_id AND f.id = e.file_id
        WHERE e.item_id = p_item AND f.current_version_id IS DISTINCT FROM e.version_id
    ) THEN
        RAISE EXCEPTION 'cited files changed since the proposal' USING ERRCODE = 'AD412';
    END IF;
    PERFORM 1 FROM public.knowledge_items c
    WHERE c.workspace_id = p_workspace AND c.kind = v_item.kind AND c.topic = v_item.topic AND c.status = 'approved'
    ORDER BY c.id FOR UPDATE;
    PERFORM pg_advisory_xact_lock(hashtextextended('knowledge-slot' || p_workspace::text || v_item.kind || coalesce(v_item.topic, ''), 0));
    v_conflicts := app.item_conflicts(p_workspace, v_item.kind, v_item.topic, v_item.supersedes, v_item.id);
    IF cardinality(v_conflicts) > 0 AND p_resolution IS NULL THEN
        RAISE EXCEPTION 'proposal conflicts with % approved item(s); choose replace or keep_both', cardinality(v_conflicts)
            USING ERRCODE = 'AD410';
    END IF;
    IF nullif(btrim(p_statement), '') IS NOT NULL AND btrim(p_statement) <> v_item.statement THEN
        INSERT INTO public.knowledge_items (workspace_id, kind, topic, statement, outcome, supersedes, actor_id, request_id,
                                            status, decided_by, decided_at, decision_note, valid_from, visibility)
        VALUES (p_workspace, v_item.kind, v_item.topic, btrim(p_statement), v_item.outcome, v_item.supersedes,
                app.current_user_id(), gen_random_uuid(), 'approved', app.current_user_id(), v_now,
                coalesce(nullif(btrim(p_note), ''), 'Edited by owner'), v_now, v_item.visibility)
        RETURNING id INTO v_result;
        INSERT INTO public.knowledge_evidence (workspace_id, item_id, file_id, version_id, session_id, event_key, handoff_id, quote)
        SELECT workspace_id, v_result, file_id, version_id, session_id, event_key, handoff_id, quote
        FROM public.knowledge_evidence WHERE item_id = p_item;
        UPDATE public.knowledge_items SET status = 'superseded', decided_by = app.current_user_id(), decided_at = v_now,
            decision_note = 'Approved with edits as ' || v_result::text, superseded_by = v_result
        WHERE id = p_item;
    ELSE
        UPDATE public.knowledge_items SET status = 'approved', decided_by = app.current_user_id(),
            decided_at = v_now, decision_note = nullif(btrim(p_note), ''), valid_from = v_now
        WHERE id = p_item;
    END IF;
    UPDATE public.knowledge_items
    SET status = 'superseded', superseded_by = v_result, invalid_at = v_now, invalidated_by = app.current_user_id()
    WHERE workspace_id = p_workspace AND status = 'approved'
      AND (id = v_item.supersedes OR (p_resolution = 'replace' AND id = ANY (v_conflicts)));
    RETURN v_result;
END
$$;

CREATE OR REPLACE FUNCTION app.retire_item(p_workspace uuid, p_item uuid, p_note text) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_item public.knowledge_items%ROWTYPE;
BEGIN
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL THEN
        RAISE EXCEPTION 'agents cannot retire' USING ERRCODE = 'insufficient_privilege';
    END IF;
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    PERFORM app.require_decider(p_workspace, v_item.visibility, v_item.actor_id);
    IF v_item.status <> 'approved' THEN RAISE EXCEPTION 'only current approved items can be retired' USING ERRCODE = 'AD409'; END IF;
    UPDATE public.knowledge_items
    SET status = 'retired', invalid_at = greatest(clock_timestamp(), valid_from), invalidated_by = app.current_user_id(),
        invalidation_note = nullif(btrim(p_note), '')
    WHERE id = p_item;
    RETURN p_item;
END
$$;
