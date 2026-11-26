-- Assistant roles (owner request 2026-11-23): low, medium and grand. Only a
-- person who owns the space gives a key a role; no assistant can issue or
-- raise a key.
--   low    = files:write   read, edit files, propose knowledge and handoffs, send messages
--   medium = review:accept low, plus approve or reject proposals and handoffs of OTHER
--                          actors (never its own) on team items
--   grand  = grand         everything the owner does with project state: accept its own
--                          proposals, retire knowledge, keep or undo auto-accepted items
--                          and handoffs, change the review mode. Never keys, members or
--                          pairing approvals.
-- An assistant never decides private items. A key with a role above low acts
-- with that role only while the person who issued it still owns the space.
-- Decisions by an assistant record which key made them (acted_agent), and
-- the activity timeline shows it.

ALTER TABLE agent_grants DROP CONSTRAINT agent_grants_scope_check;
ALTER TABLE agent_grants ADD CONSTRAINT agent_grants_scope_check
    CHECK (scope IN ('workspace:read', 'files:read', 'files:write', 'review:accept', 'grand'));

CREATE OR REPLACE FUNCTION app.scope_rank(p_scope text) RETURNS int
LANGUAGE sql IMMUTABLE SET search_path = pg_catalog, pg_temp
AS $$ SELECT CASE p_scope WHEN 'workspace:read' THEN 1 WHEN 'files:read' THEN 2 WHEN 'files:write' THEN 3
                          WHEN 'review:accept' THEN 4 WHEN 'grand' THEN 5 END $$;

-- The role of the assistant this request acts for, or NULL for a person.
-- Above low only while its issuer still owns the space.
CREATE FUNCTION app.agent_rank(p_workspace uuid) RETURNS int
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_agent uuid := nullif(current_setting('app.agent_id', true), '')::uuid;
    v_rank int;
BEGIN
    IF v_agent IS NULL THEN RETURN NULL; END IF;
    SELECT app.scope_rank(g.scope) INTO v_rank FROM public.agent_grants g
    WHERE g.id = v_agent AND g.workspace_id = p_workspace AND g.issued_by = app.current_user_id()
      AND g.revoked_at IS NULL AND g.expires_at > clock_timestamp();
    IF NOT FOUND THEN RAISE EXCEPTION 'agent context invalid' USING ERRCODE = 'insufficient_privilege'; END IF;
    IF v_rank > 3 AND NOT EXISTS (
        SELECT 1 FROM public.memberships m
        WHERE m.workspace_id = p_workspace AND m.user_id = app.current_user_id() AND m.role = 'owner'
    ) THEN
        v_rank := 3;
    END IF;
    RETURN v_rank;
END
$$;
REVOKE ALL ON FUNCTION app.agent_rank(uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.agent_rank(uuid) TO aidrive_app;

-- For assistants only (people are checked by the caller): the key needs
-- p_need, and a medium key cannot accept a record its own key proposed.
CREATE FUNCTION app.require_role(p_workspace uuid, p_need int, p_item_agent uuid) RETURNS void
LANGUAGE plpgsql STABLE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_rank int := app.agent_rank(p_workspace);
BEGIN
    IF v_rank IS NULL THEN RETURN; END IF;
    IF v_rank < p_need THEN
        RAISE EXCEPTION 'this assistant role cannot do this' USING ERRCODE = 'insufficient_privilege';
    END IF;
    IF v_rank < 5 AND p_item_agent IS NOT DISTINCT FROM nullif(current_setting('app.agent_id', true), '')::uuid THEN
        RAISE EXCEPTION 'an assistant cannot accept its own proposal' USING ERRCODE = 'insufficient_privilege';
    END IF;
END
$$;
REVOKE ALL ON FUNCTION app.require_role(uuid, int, uuid) FROM PUBLIC;

-- Writing as an assistant: any role from low up.
CREATE OR REPLACE FUNCTION app.current_writing_agent(p_workspace uuid) RETURNS uuid
LANGUAGE plpgsql STABLE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_agent uuid := nullif(current_setting('app.agent_id', true), '')::uuid;
BEGIN
    IF v_agent IS NULL THEN RETURN NULL; END IF;
    IF NOT EXISTS (
        SELECT 1 FROM public.agent_grants g
        WHERE g.id = v_agent AND g.workspace_id = p_workspace
          AND g.issued_by = app.current_user_id() AND app.scope_rank(g.scope) >= 3
          AND g.revoked_at IS NULL AND g.expires_at > clock_timestamp()
    ) THEN
        RAISE EXCEPTION 'agent context invalid' USING ERRCODE = 'insufficient_privilege';
    END IF;
    RETURN v_agent;
END
$$;

-- Owner-only actions: the person who owns the space, or a grand key of theirs.
CREATE OR REPLACE FUNCTION app.require_human_owner(p_workspace uuid) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL THEN
        PERFORM app.require_role(p_workspace, 5, NULL);
    END IF;
    PERFORM 1 FROM public.memberships m
    WHERE m.workspace_id = p_workspace AND m.workspace_id = app.current_workspace_id()
      AND m.user_id = app.current_user_id() AND m.role = 'owner'
    FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'owner required' USING ERRCODE = 'insufficient_privilege'; END IF;
END
$$;

-- Team items: the owner, or a medium or grand key of theirs (the caller
-- checks what the role allows). Private items: only their human creator.
CREATE OR REPLACE FUNCTION app.require_decider(p_workspace uuid, p_visibility text, p_actor uuid) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL THEN
        IF p_visibility <> 'team' THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
        IF coalesce(app.agent_rank(p_workspace), 0) < 4 THEN
            RAISE EXCEPTION 'this assistant role cannot decide' USING ERRCODE = 'insufficient_privilege';
        END IF;
        PERFORM 1 FROM public.memberships m
        WHERE m.workspace_id = p_workspace AND m.workspace_id = app.current_workspace_id()
          AND m.user_id = app.current_user_id() AND m.role = 'owner'
        FOR SHARE;
        IF NOT FOUND THEN RAISE EXCEPTION 'owner required' USING ERRCODE = 'insufficient_privilege'; END IF;
        RETURN;
    END IF;
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

-- Keys are issued by a person only (never in an assistant's context), and
-- only an owner issues them (lock_grant_owner).
CREATE OR REPLACE FUNCTION app.issue_agent_grant(p_workspace uuid, p_id uuid, p_name text, p_hash bytea, p_scope text)
RETURNS void LANGUAGE plpgsql VOLATILE SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL THEN
        RAISE EXCEPTION 'assistants cannot issue keys' USING ERRCODE = 'insufficient_privilege';
    END IF;
    PERFORM app.lock_grant_owner(p_workspace);
    INSERT INTO public.agent_grants (id, workspace_id, issued_by, name, token_hash, scope)
    VALUES (p_id, p_workspace, app.current_user_id(), btrim(p_name), p_hash, p_scope);
END
$$;

-- decided_agent: the key that approved or rejected; acted_agent: the key that
-- last kept, undid or retired. Two columns so a later retire does not hide
-- who approved.
ALTER TABLE knowledge_items ADD COLUMN decided_agent uuid REFERENCES agent_grants(id),
                            ADD COLUMN acted_agent uuid REFERENCES agent_grants(id);
ALTER TABLE handoffs ADD COLUMN decided_agent uuid REFERENCES agent_grants(id),
                     ADD COLUMN acted_agent uuid REFERENCES agent_grants(id);

-- Which assistant key made a decision (approve, reject, keep, undo, retire).
-- Automatic acceptance is not a decision by a role and is not stamped.
-- One function per table: PL/pgSQL resolves every NEW field it names.
CREATE FUNCTION app.stamp_item_acted_agent() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_agent uuid := nullif(current_setting('app.agent_id', true), '')::uuid;
BEGIN
    IF v_agent IS NULL THEN RETURN NEW; END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.status = 'approved' AND NOT NEW.auto_accepted AND NEW.decided_at IS NOT NULL THEN
            NEW.decided_agent := v_agent;
        END IF;
        RETURN NEW;
    END IF;
    IF NEW.decided_at IS DISTINCT FROM OLD.decided_at AND NOT NEW.auto_accepted THEN
        NEW.decided_agent := v_agent;
    END IF;
    IF NEW.confirmed_at IS DISTINCT FROM OLD.confirmed_at OR (NEW.status = 'retired' AND OLD.status <> 'retired') THEN
        NEW.acted_agent := v_agent;
    END IF;
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION app.stamp_item_acted_agent() FROM PUBLIC;
CREATE TRIGGER knowledge_items_acted_agent BEFORE INSERT OR UPDATE ON knowledge_items
FOR EACH ROW EXECUTE FUNCTION app.stamp_item_acted_agent();

CREATE FUNCTION app.stamp_handoff_acted_agent() RETURNS trigger
LANGUAGE plpgsql SET search_path = pg_catalog, pg_temp
AS $$
DECLARE v_agent uuid := nullif(current_setting('app.agent_id', true), '')::uuid;
BEGIN
    IF v_agent IS NULL OR TG_OP = 'INSERT' THEN RETURN NEW; END IF;
    IF NEW.decided_at IS DISTINCT FROM OLD.decided_at AND NOT NEW.auto_accepted THEN
        NEW.decided_agent := v_agent;
    END IF;
    IF NEW.confirmed_at IS DISTINCT FROM OLD.confirmed_at OR NEW.undone_at IS DISTINCT FROM OLD.undone_at THEN
        NEW.acted_agent := v_agent;
    END IF;
    RETURN NEW;
END
$$;
REVOKE ALL ON FUNCTION app.stamp_handoff_acted_agent() FROM PUBLIC;
CREATE TRIGGER handoffs_acted_agent BEFORE UPDATE ON handoffs
FOR EACH ROW EXECUTE FUNCTION app.stamp_handoff_acted_agent();

-- Medium and grand keys may decide team proposals; medium never its own.
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
    IF p_decision NOT IN ('approved', 'rejected') OR p_resolution NOT IN ('replace', 'keep_both') THEN
        RAISE EXCEPTION 'invalid decision' USING ERRCODE = 'check_violation';
    END IF;
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    PERFORM app.require_decider(p_workspace, v_item.visibility, v_item.actor_id);
    PERFORM app.require_role(p_workspace, 4, v_item.agent_id);
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

-- Retiring, keeping and undoing are grand (owner-level) actions.
CREATE OR REPLACE FUNCTION app.retire_item(p_workspace uuid, p_item uuid, p_note text) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_item public.knowledge_items%ROWTYPE;
BEGIN
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    PERFORM app.require_decider(p_workspace, v_item.visibility, v_item.actor_id);
    PERFORM app.require_role(p_workspace, 5, v_item.agent_id);
    IF v_item.status <> 'approved' THEN RAISE EXCEPTION 'only current approved items can be retired' USING ERRCODE = 'AD409'; END IF;
    UPDATE public.knowledge_items
    SET status = 'retired', invalid_at = greatest(clock_timestamp(), valid_from), invalidated_by = app.current_user_id(),
        invalidation_note = nullif(btrim(p_note), '')
    WHERE id = p_item;
    RETURN p_item;
END
$$;

CREATE OR REPLACE FUNCTION app.confirm_item(p_workspace uuid, p_item uuid) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_item public.knowledge_items%ROWTYPE;
BEGIN
    -- Outsiders learn nothing, not even that the record exists.
    IF NOT app.has_role(p_workspace, 'viewer') THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    PERFORM app.require_decider(p_workspace, v_item.visibility, v_item.actor_id);
    PERFORM app.require_role(p_workspace, 5, v_item.agent_id);
    IF NOT v_item.auto_accepted OR v_item.confirmed_at IS NOT NULL OR v_item.status <> 'approved' THEN
        RAISE EXCEPTION 'only unchecked auto-accepted items can be kept' USING ERRCODE = 'AD409';
    END IF;
    UPDATE public.knowledge_items SET confirmed_by = app.current_user_id(), confirmed_at = clock_timestamp() WHERE id = p_item;
    RETURN p_item;
END
$$;

CREATE OR REPLACE FUNCTION app.undo_item(p_workspace uuid, p_item uuid, p_note text) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_item public.knowledge_items%ROWTYPE;
BEGIN
    -- Outsiders learn nothing, not even that the record exists.
    IF NOT app.has_role(p_workspace, 'viewer') THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    PERFORM app.require_decider(p_workspace, v_item.visibility, v_item.actor_id);
    PERFORM app.require_role(p_workspace, 5, v_item.agent_id);
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

-- Medium keys decide other actors' handoffs; grand keys any handoff.
CREATE OR REPLACE FUNCTION app.decide_handoff(p_workspace uuid, p_handoff uuid, p_decision text, p_note text)
RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_status text;
    v_current uuid;
BEGIN
    IF nullif(current_setting('app.agent_id', true), '') IS NULL THEN
        PERFORM app.require_human_owner(p_workspace);
    ELSE
        PERFORM app.require_role(p_workspace, 4,
            (SELECT h.agent_id FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.id = p_handoff));
    END IF;
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

-- The timeline names the assistant key that made a decision.
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
               h.decided_by, CASE WHEN h.auto_accepted THEN h.agent_id ELSE h.decided_agent END, left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.decided_at IS NOT NULL
        UNION ALL
        SELECT h.confirmed_at, 'handoff.kept', h.confirmed_by, h.acted_agent, left(h.summary, 200), NULL, NULL, NULL, h.id
        FROM public.handoffs h WHERE h.workspace_id = p_workspace AND h.confirmed_at IS NOT NULL
        UNION ALL
        SELECT h.undone_at, 'handoff.undone', h.undone_by, h.acted_agent, left(h.summary, 200), NULL, NULL, NULL, h.id
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
               k.decided_by, CASE WHEN k.auto_accepted THEN k.agent_id ELSE k.decided_agent END, left(k.statement, 200), NULL, NULL, NULL, NULL
        FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND (k.valid_from IS NOT NULL OR k.status = 'rejected')
          AND NOT app.is_derived_item(k.decision_note) AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
        UNION ALL
        SELECT k.confirmed_at, 'knowledge.kept', k.confirmed_by, k.acted_agent, left(k.statement, 200), NULL, NULL, NULL, NULL
        FROM public.knowledge_items k
        WHERE k.workspace_id = p_workspace AND k.confirmed_at IS NOT NULL AND NOT app.is_derived_item(k.decision_note)
          AND app.can_see(k.workspace_id, k.visibility, k.actor_id)
        UNION ALL
        SELECT k.invalid_at,
               CASE WHEN k.auto_accepted AND k.confirmed_at IS NULL THEN 'knowledge.undone' ELSE 'knowledge.retired' END,
               k.invalidated_by, k.acted_agent, left(k.statement, 200), NULL, NULL, NULL, NULL
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
        UNION ALL
        SELECT a.created_at, 'artifact.registered', a.actor_id, a.agent_id, left(a.path, 200), NULL, NULL, NULL, NULL
        FROM public.artifacts a WHERE a.workspace_id = p_workspace
        UNION ALL
        SELECT a.closed_at, 'artifact.' || a.status, a.closed_by, a.closed_agent, left(a.path, 200), NULL, NULL, NULL, NULL
        FROM public.artifacts a WHERE a.workspace_id = p_workspace AND a.closed_at IS NOT NULL
    ) e
    LEFT JOIN public.users u ON u.id = e.actor
    LEFT JOIN public.agent_grants g ON g.id = e.agent_id
    WHERE p_before IS NULL OR e.at < p_before
    ORDER BY e.at DESC, e.kind
    LIMIT least(greatest(coalesce(p_limit, 50), 1), 100);
END
$$;
