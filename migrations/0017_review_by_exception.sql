-- Approval by exception (ROADMAP "T1 approval by exception: decision").
-- Per-space review mode. In 'exceptions' a proposal is accepted at once when
-- nothing about it needs the owner: current evidence, no conflict, not a
-- keystone, not a correction. Everything else waits in the inbox as before.
-- Auto-accepted records are flagged, attributed to their proposer, and the
-- owner can keep (confirm) or undo them in one action. 'strict' is the old
-- behavior and the default for spaces without a setting, so this migration
-- changes no existing record; the API turns 'exceptions' on for new spaces.

CREATE TABLE workspace_settings (
    workspace_id uuid PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
    review_mode text NOT NULL CHECK (review_mode IN ('exceptions', 'strict')),
    updated_by uuid NOT NULL REFERENCES users(id),
    updated_at timestamptz NOT NULL DEFAULT clock_timestamp()
);
ALTER TABLE workspace_settings ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON workspace_settings FROM PUBLIC, aidrive_app;
GRANT SELECT ON workspace_settings TO aidrive_app;
CREATE POLICY workspace_settings_read ON workspace_settings FOR SELECT USING (app.has_role(workspace_id, 'viewer'));

CREATE FUNCTION app.review_mode(p_workspace uuid) RETURNS text
LANGUAGE sql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$ SELECT coalesce((SELECT s.review_mode FROM public.workspace_settings s WHERE s.workspace_id = p_workspace), 'strict') $$;
REVOKE ALL ON FUNCTION app.review_mode(uuid) FROM PUBLIC;

-- Only the human owner changes how the space is reviewed.
CREATE FUNCTION app.set_review_mode(p_workspace uuid, p_mode text) RETURNS text
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    PERFORM app.require_human_owner(p_workspace);
    IF p_mode IS NULL OR p_mode NOT IN ('exceptions', 'strict') THEN
        RAISE EXCEPTION 'invalid review mode' USING ERRCODE = 'check_violation';
    END IF;
    INSERT INTO public.workspace_settings (workspace_id, review_mode, updated_by)
    VALUES (p_workspace, p_mode, app.current_user_id())
    ON CONFLICT (workspace_id) DO UPDATE
    SET review_mode = EXCLUDED.review_mode, updated_by = EXCLUDED.updated_by, updated_at = clock_timestamp();
    RETURN p_mode;
END
$$;
REVOKE ALL ON FUNCTION app.set_review_mode(uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.set_review_mode(uuid, text) TO aidrive_app;

ALTER TABLE knowledge_items
    ADD COLUMN auto_accepted boolean NOT NULL DEFAULT false,
    ADD COLUMN confirmed_by uuid REFERENCES users(id),
    ADD COLUMN confirmed_at timestamptz,
    ADD CONSTRAINT knowledge_items_auto_check CHECK (NOT auto_accepted OR valid_from IS NOT NULL),
    ADD CONSTRAINT knowledge_items_confirmed_check CHECK ((confirmed_by IS NULL) = (confirmed_at IS NULL) AND (confirmed_at IS NULL OR auto_accepted));

ALTER TABLE handoffs
    ADD COLUMN auto_accepted boolean NOT NULL DEFAULT false,
    ADD COLUMN replaced uuid,
    ADD COLUMN confirmed_by uuid REFERENCES users(id),
    ADD COLUMN confirmed_at timestamptz,
    ADD COLUMN undone_by uuid REFERENCES users(id),
    ADD COLUMN undone_at timestamptz,
    ADD FOREIGN KEY (workspace_id, replaced) REFERENCES handoffs(workspace_id, id),
    ADD CONSTRAINT handoffs_auto_check CHECK (NOT auto_accepted OR status <> 'proposed'),
    ADD CONSTRAINT handoffs_confirmed_check CHECK ((confirmed_by IS NULL) = (confirmed_at IS NULL) AND (confirmed_at IS NULL OR auto_accepted)),
    ADD CONSTRAINT handoffs_undone_check CHECK ((undone_by IS NULL) = (undone_at IS NULL)
        AND (undone_at IS NULL OR (auto_accepted AND status = 'rejected' AND confirmed_at IS NULL)));

-- Accept one just-proposed item when nothing about it needs the owner.
-- Internal: called by propose_item only, never granted to the app role.
CREATE FUNCTION app.auto_accept_item(p_workspace uuid, p_item uuid) RETURNS boolean
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_item public.knowledge_items%ROWTYPE;
    v_now timestamptz;
BEGIN
    IF app.review_mode(p_workspace) <> 'exceptions' THEN RETURN false; END IF;
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND OR v_item.status <> 'proposed' OR v_item.kind = 'keystone' OR v_item.supersedes IS NOT NULL
       OR NOT EXISTS (SELECT 1 FROM public.knowledge_evidence e WHERE e.item_id = p_item) THEN
        RETURN false;
    END IF;
    PERFORM 1 FROM public.files f JOIN public.knowledge_evidence e ON e.workspace_id = f.workspace_id AND e.file_id = f.id
    WHERE e.item_id = p_item ORDER BY f.id FOR SHARE OF f;
    IF EXISTS (
        SELECT 1 FROM public.knowledge_evidence e JOIN public.files f ON f.workspace_id = e.workspace_id AND f.id = e.file_id
        WHERE e.item_id = p_item AND f.current_version_id IS DISTINCT FROM e.version_id
    ) THEN
        RETURN false;
    END IF;
    -- Same slot lock as decide_item, so two proposals for one topic cannot
    -- both see an empty slot and both be accepted.
    PERFORM 1 FROM public.knowledge_items c
    WHERE c.workspace_id = p_workspace AND c.kind = v_item.kind AND c.topic = v_item.topic AND c.status = 'approved'
    ORDER BY c.id FOR UPDATE;
    PERFORM pg_advisory_xact_lock(hashtextextended('knowledge-slot' || p_workspace::text || v_item.kind || coalesce(v_item.topic, ''), 0));
    IF cardinality(app.item_conflicts(p_workspace, v_item.kind, v_item.topic, NULL, p_item)) > 0 THEN
        RETURN false;
    END IF;
    v_now := clock_timestamp();
    UPDATE public.knowledge_items
    SET status = 'approved', decided_by = v_item.actor_id, decided_at = v_now, valid_from = v_now, auto_accepted = true,
        decision_note = 'Auto-accepted: current evidence, no conflict'
    WHERE id = p_item;
    RETURN true;
END
$$;
REVOKE ALL ON FUNCTION app.auto_accept_item(uuid, uuid) FROM PUBLIC;

-- 0015's propose_item plus the auto-accept step for new proposals (a
-- replayed request returns the existing item unchanged).
CREATE OR REPLACE FUNCTION app.propose_item(
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
    PERFORM app.auto_accept_item(p_workspace, v_id);
    RETURN v_id;
END
$$;

-- Accept one just-recorded handoff when every cited file is still at the
-- cited version. One approved handoff at a time: it replaces the current
-- one, which is remembered so an undo can bring it back.
CREATE FUNCTION app.auto_accept_handoff(p_workspace uuid, p_handoff uuid) RETURNS boolean
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_handoff public.handoffs%ROWTYPE;
    v_current uuid;
BEGIN
    IF app.review_mode(p_workspace) <> 'exceptions' THEN RETURN false; END IF;
    SELECT * INTO v_handoff FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.id = p_handoff FOR UPDATE;
    IF NOT FOUND OR v_handoff.status <> 'proposed'
       OR NOT EXISTS (SELECT 1 FROM public.handoff_sources s WHERE s.handoff_id = p_handoff) THEN
        RETURN false;
    END IF;
    PERFORM 1 FROM public.files f JOIN public.handoff_sources s ON s.workspace_id = f.workspace_id AND s.file_id = f.id
    WHERE s.handoff_id = p_handoff ORDER BY f.id FOR SHARE OF f;
    IF EXISTS (
        SELECT 1 FROM public.handoff_sources s JOIN public.files f ON f.workspace_id = s.workspace_id AND f.id = s.file_id
        WHERE s.handoff_id = p_handoff AND f.current_version_id IS DISTINCT FROM s.version_id
    ) THEN
        RETURN false;
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('handoff-current' || p_workspace::text, 0));
    SELECT h.id INTO v_current FROM public.handoffs h
    WHERE h.workspace_id = p_workspace AND h.status = 'approved' ORDER BY h.decided_at DESC, h.id LIMIT 1 FOR UPDATE;
    UPDATE public.handoffs SET status = 'superseded' WHERE workspace_id = p_workspace AND status = 'approved';
    UPDATE public.handoffs
    SET status = 'approved', decided_by = v_handoff.actor_id, decided_at = clock_timestamp(), auto_accepted = true,
        replaced = v_current, decision_note = 'Auto-accepted: cited files unchanged'
    WHERE id = p_handoff;
    RETURN true;
END
$$;
REVOKE ALL ON FUNCTION app.auto_accept_handoff(uuid, uuid) FROM PUBLIC;

CREATE OR REPLACE FUNCTION app.record_handoff(
    p_workspace uuid, p_request uuid, p_summary text, p_decisions text[], p_next_step text, p_sources jsonb
) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_id uuid;
    v_prior public.handoffs%ROWTYPE;
    v_agent uuid;
    v_decisions text[] := coalesce(p_decisions, '{}');
BEGIN
    PERFORM app.lock_file_access(p_workspace, true);
    v_agent := app.current_writing_agent(p_workspace);
    IF p_request IS NULL OR jsonb_typeof(coalesce(p_sources, '[]'::jsonb)) <> 'array'
       OR jsonb_array_length(coalesce(p_sources, '[]'::jsonb)) > 20
       OR EXISTS (SELECT 1 FROM unnest(v_decisions) d WHERE length(btrim(d)) NOT BETWEEN 1 AND 1000) THEN
        RAISE EXCEPTION 'invalid handoff' USING ERRCODE = 'check_violation';
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('handoff' || p_workspace::text || app.current_user_id()::text || p_request::text, 0));
    SELECT * INTO v_prior FROM public.handoffs h
    WHERE h.workspace_id = p_workspace AND h.actor_id = app.current_user_id() AND h.request_id = p_request;
    IF FOUND THEN
        IF v_prior.summary IS DISTINCT FROM btrim(p_summary) OR v_prior.next_step IS DISTINCT FROM btrim(p_next_step)
           OR v_prior.decisions IS DISTINCT FROM v_decisions OR v_prior.agent_id IS DISTINCT FROM v_agent THEN
            RAISE EXCEPTION 'request ID reused with different input' USING ERRCODE = 'AD409';
        END IF;
        RETURN v_prior.id;
    END IF;
    INSERT INTO public.handoffs (workspace_id, actor_id, agent_id, request_id, summary, decisions, next_step)
    VALUES (p_workspace, app.current_user_id(), v_agent, p_request, btrim(p_summary), v_decisions, btrim(p_next_step))
    RETURNING id INTO v_id;
    INSERT INTO public.handoff_sources (workspace_id, handoff_id, file_id, version_id)
    SELECT p_workspace, v_id, (s->>'file_id')::uuid, (s->>'version_id')::uuid
    FROM jsonb_array_elements(coalesce(p_sources, '[]'::jsonb)) s;
    PERFORM app.auto_accept_handoff(p_workspace, v_id);
    RETURN v_id;
END
$$;

-- Decisions of an auto-accepted handoff are auto-accepted items too.
CREATE OR REPLACE FUNCTION app.items_from_approved_handoff() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    d text;
    v_item uuid;
BEGIN
    IF NEW.status = 'approved' AND OLD.status = 'proposed' THEN
        FOREACH d IN ARRAY NEW.decisions LOOP
            INSERT INTO public.knowledge_items (workspace_id, kind, statement, actor_id, agent_id, request_id,
                                                status, decided_by, decided_at, decision_note, valid_from, auto_accepted)
            VALUES (NEW.workspace_id, 'decision', d, NEW.actor_id, NEW.agent_id, gen_random_uuid(),
                    'approved', NEW.decided_by, NEW.decided_at,
                    CASE WHEN NEW.auto_accepted THEN 'From auto-accepted handoff' ELSE 'From approved handoff' END,
                    NEW.decided_at, NEW.auto_accepted)
            RETURNING id INTO v_item;
            INSERT INTO public.knowledge_evidence (workspace_id, item_id, handoff_id, quote)
            VALUES (NEW.workspace_id, v_item, NEW.id, left(NEW.summary, 500));
            INSERT INTO public.knowledge_evidence (workspace_id, item_id, file_id, version_id)
            SELECT NEW.workspace_id, v_item, s.file_id, s.version_id FROM public.handoff_sources s WHERE s.handoff_id = NEW.id;
        END LOOP;
    END IF;
    RETURN NULL;
END
$$;

-- Keep: the owner (or, for private items, their creator) checked it.
CREATE FUNCTION app.confirm_item(p_workspace uuid, p_item uuid) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_item public.knowledge_items%ROWTYPE;
BEGIN
    -- Outsiders learn nothing, not even that the record exists.
    IF NOT app.has_role(p_workspace, 'viewer') THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL THEN
        RAISE EXCEPTION 'agents cannot confirm' USING ERRCODE = 'insufficient_privilege';
    END IF;
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    PERFORM app.require_decider(p_workspace, v_item.visibility, v_item.actor_id);
    IF NOT v_item.auto_accepted OR v_item.confirmed_at IS NOT NULL OR v_item.status <> 'approved' THEN
        RAISE EXCEPTION 'only unchecked auto-accepted items can be kept' USING ERRCODE = 'AD409';
    END IF;
    UPDATE public.knowledge_items SET confirmed_by = app.current_user_id(), confirmed_at = clock_timestamp() WHERE id = p_item;
    RETURN p_item;
END
$$;
REVOKE ALL ON FUNCTION app.confirm_item(uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.confirm_item(uuid, uuid) TO aidrive_app;

-- Undo: the item stops being current knowledge. It is retired, not
-- rejected, because assistants may already have read it; as_of keeps it.
CREATE FUNCTION app.undo_item(p_workspace uuid, p_item uuid, p_note text) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_item public.knowledge_items%ROWTYPE;
BEGIN
    -- Outsiders learn nothing, not even that the record exists.
    IF NOT app.has_role(p_workspace, 'viewer') THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL THEN
        RAISE EXCEPTION 'agents cannot undo' USING ERRCODE = 'insufficient_privilege';
    END IF;
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    PERFORM app.require_decider(p_workspace, v_item.visibility, v_item.actor_id);
    IF NOT v_item.auto_accepted OR v_item.confirmed_at IS NOT NULL OR v_item.status <> 'approved' THEN
        RAISE EXCEPTION 'only unchecked auto-accepted items can be undone' USING ERRCODE = 'AD409';
    END IF;
    UPDATE public.knowledge_items
    SET status = 'retired', invalid_at = greatest(clock_timestamp(), valid_from), invalidated_by = app.current_user_id(),
        invalidation_note = coalesce(nullif(btrim(p_note), ''), 'Undone by owner')
    WHERE id = p_item;
    RETURN p_item;
END
$$;
REVOKE ALL ON FUNCTION app.undo_item(uuid, uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.undo_item(uuid, uuid, text) TO aidrive_app;

CREATE FUNCTION app.confirm_handoff(p_workspace uuid, p_handoff uuid) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_handoff public.handoffs%ROWTYPE;
BEGIN
    -- Outsiders learn nothing, not even that the record exists.
    IF NOT app.has_role(p_workspace, 'viewer') THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    PERFORM app.require_human_owner(p_workspace);
    SELECT * INTO v_handoff FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.id = p_handoff FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    IF NOT v_handoff.auto_accepted OR v_handoff.confirmed_at IS NOT NULL OR v_handoff.status NOT IN ('approved', 'superseded') THEN
        RAISE EXCEPTION 'only unchecked auto-accepted handoffs can be kept' USING ERRCODE = 'AD409';
    END IF;
    UPDATE public.handoffs SET confirmed_by = app.current_user_id(), confirmed_at = clock_timestamp() WHERE id = p_handoff;
    UPDATE public.knowledge_items k SET confirmed_by = app.current_user_id(), confirmed_at = clock_timestamp()
    WHERE k.workspace_id = p_workspace AND k.auto_accepted AND k.confirmed_at IS NULL AND k.status = 'approved'
      AND EXISTS (SELECT 1 FROM public.knowledge_evidence e WHERE e.item_id = k.id AND e.handoff_id = p_handoff);
    RETURN p_handoff;
END
$$;
REVOKE ALL ON FUNCTION app.confirm_handoff(uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.confirm_handoff(uuid, uuid) TO aidrive_app;

-- Undo a handoff: it becomes rejected, its unchecked decisions are retired,
-- and if it was the current handoff the one it replaced becomes current again.
CREATE FUNCTION app.undo_handoff(p_workspace uuid, p_handoff uuid, p_note text) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_handoff public.handoffs%ROWTYPE;
    v_now timestamptz := clock_timestamp();
BEGIN
    -- Outsiders learn nothing, not even that the record exists.
    IF NOT app.has_role(p_workspace, 'viewer') THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    PERFORM app.require_human_owner(p_workspace);
    PERFORM pg_advisory_xact_lock(hashtextextended('handoff-current' || p_workspace::text, 0));
    SELECT * INTO v_handoff FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.id = p_handoff FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    IF NOT v_handoff.auto_accepted OR v_handoff.confirmed_at IS NOT NULL OR v_handoff.status NOT IN ('approved', 'superseded') THEN
        RAISE EXCEPTION 'only unchecked auto-accepted handoffs can be undone' USING ERRCODE = 'AD409';
    END IF;
    UPDATE public.handoffs
    SET status = 'rejected', undone_by = app.current_user_id(), undone_at = v_now,
        decision_note = coalesce(nullif(btrim(p_note), ''), 'Undone by owner')
    WHERE id = p_handoff;
    UPDATE public.knowledge_items k
    SET status = 'retired', invalid_at = greatest(v_now, k.valid_from), invalidated_by = app.current_user_id(),
        invalidation_note = 'Handoff undone by owner'
    WHERE k.workspace_id = p_workspace AND k.auto_accepted AND k.confirmed_at IS NULL AND k.status = 'approved'
      AND EXISTS (SELECT 1 FROM public.knowledge_evidence e WHERE e.item_id = k.id AND e.handoff_id = p_handoff);
    IF v_handoff.status = 'approved' AND v_handoff.replaced IS NOT NULL THEN
        UPDATE public.handoffs SET status = 'approved'
        WHERE workspace_id = p_workspace AND id = v_handoff.replaced AND status = 'superseded'
          AND NOT EXISTS (SELECT 1 FROM public.handoffs a WHERE a.workspace_id = p_workspace AND a.status = 'approved');
    END IF;
    RETURN p_handoff;
END
$$;
REVOKE ALL ON FUNCTION app.undo_handoff(uuid, uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.undo_handoff(uuid, uuid, text) TO aidrive_app;

-- The owner approving a handoff while an auto-accepted one is current also
-- remembers what it replaced, so the timeline stays explainable.
CREATE OR REPLACE FUNCTION app.decide_handoff(p_workspace uuid, p_handoff uuid, p_decision text, p_note text)
RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_status text;
    v_current uuid;
BEGIN
    PERFORM app.require_human_owner(p_workspace);
    IF p_decision NOT IN ('approved', 'rejected') THEN
        RAISE EXCEPTION 'invalid decision' USING ERRCODE = 'check_violation';
    END IF;
    SELECT h.status INTO v_status FROM public.handoffs h
    WHERE h.workspace_id = p_workspace AND h.id = p_handoff FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    IF v_status <> 'proposed' THEN RAISE EXCEPTION 'already decided' USING ERRCODE = 'AD409'; END IF;
    IF p_decision = 'approved' THEN
        PERFORM 1 FROM public.files f
        JOIN public.handoff_sources s ON s.workspace_id = f.workspace_id AND s.file_id = f.id
        WHERE s.handoff_id = p_handoff
        ORDER BY f.id
        FOR SHARE OF f;
        IF EXISTS (
            SELECT 1 FROM public.handoff_sources s
            JOIN public.files f ON f.workspace_id = s.workspace_id AND f.id = s.file_id
            WHERE s.handoff_id = p_handoff AND f.current_version_id IS DISTINCT FROM s.version_id
        ) THEN
            RAISE EXCEPTION 'cited sources changed since the handoff' USING ERRCODE = 'AD412';
        END IF;
        PERFORM pg_advisory_xact_lock(hashtextextended('handoff-current' || p_workspace::text, 0));
        SELECT h.id INTO v_current FROM public.handoffs h
        WHERE h.workspace_id = p_workspace AND h.status = 'approved' ORDER BY h.decided_at DESC, h.id LIMIT 1;
        UPDATE public.handoffs SET status = 'superseded'
        WHERE workspace_id = p_workspace AND status = 'approved';
    END IF;
    UPDATE public.handoffs
    SET status = p_decision, decided_by = app.current_user_id(), decided_at = clock_timestamp(),
        decision_note = nullif(btrim(p_note), ''), replaced = v_current
    WHERE workspace_id = p_workspace AND id = p_handoff;
END
$$;

-- Decisions copied from a handoff count with their handoff, not twice.
CREATE FUNCTION app.is_derived_item(p_note text) RETURNS boolean
LANGUAGE sql IMMUTABLE SET search_path = pg_catalog, pg_temp
AS $$ SELECT coalesce(p_note IN ('From approved handoff', 'From auto-accepted handoff', 'From approved handoff (backfill)'), false) $$;
REVOKE ALL ON FUNCTION app.is_derived_item(text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.is_derived_item(text) TO aidrive_app;

-- Timeline: 0009's and 0016's events (messages), auto-acceptance kept apart from owner approval,
-- plus knowledge events (visible items only) and keep/undo.
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
               CASE WHEN h.auto_accepted THEN 'handoff.auto_accepted'
                    WHEN h.status = 'rejected' THEN 'handoff.rejected' ELSE 'handoff.approved' END,
               h.decided_by, CASE WHEN h.auto_accepted THEN h.agent_id END, left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.decided_at IS NOT NULL
        UNION ALL
        SELECT h.confirmed_at, 'handoff.kept', h.confirmed_by, NULL, left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.confirmed_at IS NOT NULL
        UNION ALL
        SELECT h.undone_at, 'handoff.undone', h.undone_by, NULL, left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.undone_at IS NOT NULL
        UNION ALL
        SELECT k.created_at, 'knowledge.proposed', k.actor_id, k.agent_id, left(k.statement, 200), NULL, NULL, NULL, NULL
        FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND NOT app.is_derived_item(k.decision_note)
          AND NOT EXISTS (SELECT 1 FROM public.knowledge_items o WHERE o.workspace_id = k.workspace_id AND o.superseded_by = k.id
                          AND o.decision_note = 'Approved with edits as ' || k.id::text)
          AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
        UNION ALL
        SELECT k.decided_at,
               CASE WHEN k.auto_accepted THEN 'knowledge.auto_accepted'
                    WHEN k.status = 'rejected' THEN 'knowledge.rejected' ELSE 'knowledge.approved' END,
               k.decided_by, CASE WHEN k.auto_accepted THEN k.agent_id END, left(k.statement, 200), NULL, NULL, NULL, NULL
        FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND (k.valid_from IS NOT NULL OR k.status = 'rejected')
          AND NOT app.is_derived_item(k.decision_note) AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
        UNION ALL
        SELECT k.confirmed_at, 'knowledge.kept', k.confirmed_by, NULL, left(k.statement, 200), NULL, NULL, NULL, NULL
        FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND k.confirmed_at IS NOT NULL AND NOT app.is_derived_item(k.decision_note)
          AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
        UNION ALL
        SELECT k.invalid_at,
               CASE WHEN k.auto_accepted AND k.confirmed_at IS NULL THEN 'knowledge.undone' ELSE 'knowledge.retired' END,
               k.invalidated_by, NULL, left(k.statement, 200), NULL, NULL, NULL, NULL
        FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND k.status = 'retired'
          AND k.invalidation_note IS DISTINCT FROM 'Handoff undone by owner'
          AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
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

-- Review metric per UTC day: what the owner approved by hand, what was
-- accepted automatically, and what of that the owner kept or undid.
-- Undone = an unchecked auto-accepted item retired, or a handoff undone.
-- Counts include only records the caller can see.
CREATE FUNCTION app.review_metrics(p_workspace uuid, p_days int)
RETURNS TABLE (day date, owner_approved bigint, auto_accepted bigint, kept bigint, undone bigint, owner_rejected bigint)
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_since timestamptz;
BEGIN
    IF NOT app.has_role(p_workspace, 'viewer') THEN
        RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found';
    END IF;
    v_since := date_trunc('day', clock_timestamp() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'
               - make_interval(days => least(greatest(coalesce(p_days, 7), 1), 90) - 1);
    RETURN QUERY
    WITH ev AS (
        SELECT h.decided_at AS at,
               CASE WHEN h.auto_accepted THEN 'auto' WHEN h.status = 'rejected' THEN 'rejected' ELSE 'owner' END AS kind
        FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.decided_at >= v_since
        UNION ALL
        SELECT h.confirmed_at, 'kept' FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.confirmed_at >= v_since
        UNION ALL
        SELECT h.undone_at, 'undone' FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.undone_at >= v_since
        UNION ALL
        SELECT k.decided_at, CASE WHEN k.auto_accepted THEN 'auto' WHEN k.status = 'rejected' THEN 'rejected' ELSE 'owner' END
        FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND k.decided_at >= v_since AND (k.valid_from IS NOT NULL OR k.status = 'rejected')
          AND NOT app.is_derived_item(k.decision_note) AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
        UNION ALL
        SELECT k.confirmed_at, 'kept' FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND k.confirmed_at >= v_since
          AND NOT app.is_derived_item(k.decision_note) AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
        UNION ALL
        SELECT k.invalid_at, 'undone' FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND k.invalid_at >= v_since AND k.status = 'retired' AND k.auto_accepted
          AND k.confirmed_at IS NULL AND NOT app.is_derived_item(k.decision_note)
          AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
    )
    SELECT d::date,
           count(*) FILTER (WHERE ev.kind = 'owner'), count(*) FILTER (WHERE ev.kind = 'auto'),
           count(*) FILTER (WHERE ev.kind = 'kept'), count(*) FILTER (WHERE ev.kind = 'undone'),
           count(*) FILTER (WHERE ev.kind = 'rejected')
    FROM generate_series(v_since AT TIME ZONE 'UTC', clock_timestamp() AT TIME ZONE 'UTC', interval '1 day') d
    LEFT JOIN ev ON (ev.at AT TIME ZONE 'UTC')::date = d::date
    GROUP BY d::date ORDER BY d::date;
END
$$;
REVOKE ALL ON FUNCTION app.review_metrics(uuid, int) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.review_metrics(uuid, int) TO aidrive_app;
