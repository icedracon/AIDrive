-- Project knowledge (docs/research/ai-memory-findings.md, plan M1).
-- Items are immutable statements with typed kinds and evidence pinned to an
-- exact file version, a captured session event or a handoff. Assistants
-- propose; only the human owner approves, rejects or corrects. A correction
-- is a new item that supersedes the old one (kept for history, hidden from
-- current recall). Approval is refused when cited file evidence moved on;
-- staleness of approved items is computed on read like handoffs.

CREATE TABLE knowledge_items (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('decision', 'fact', 'constraint', 'preference', 'gotcha', 'attempt', 'keystone')),
    topic text CHECK (topic IS NULL OR length(topic) BETWEEN 1 AND 120),
    statement text NOT NULL CHECK (length(btrim(statement)) BETWEEN 1 AND 2000),
    outcome text CHECK (outcome IN ('failed', 'partial', 'worked')),
    status text NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed', 'approved', 'rejected', 'superseded')),
    supersedes uuid,
    actor_id uuid NOT NULL REFERENCES users(id),
    agent_id uuid REFERENCES agent_grants(id),
    request_id uuid NOT NULL,
    decided_by uuid REFERENCES users(id),
    decided_at timestamptz,
    decision_note text CHECK (length(decision_note) <= 2000),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    search tsvector GENERATED ALWAYS AS (to_tsvector('simple', coalesce(topic, '') || ' ' || statement)) STORED,
    UNIQUE (workspace_id, id),
    UNIQUE (workspace_id, actor_id, request_id),
    FOREIGN KEY (workspace_id, supersedes) REFERENCES knowledge_items(workspace_id, id),
    CHECK ((kind = 'attempt') = (outcome IS NOT NULL)),
    CHECK ((status = 'proposed') = (decided_by IS NULL))
);
CREATE INDEX knowledge_items_status_idx ON knowledge_items (workspace_id, status, kind);
CREATE INDEX knowledge_items_search_idx ON knowledge_items USING gin (search);

CREATE TABLE knowledge_evidence (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    workspace_id uuid NOT NULL,
    item_id uuid NOT NULL,
    file_id uuid,
    version_id uuid,
    session_id uuid,
    event_key text,
    handoff_id uuid,
    quote text CHECK (length(quote) <= 500),
    FOREIGN KEY (workspace_id, item_id) REFERENCES knowledge_items(workspace_id, id) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id, file_id, version_id) REFERENCES file_versions(workspace_id, file_id, id) ON DELETE CASCADE,
    FOREIGN KEY (session_id, event_key) REFERENCES session_events(session_id, event_key) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id, handoff_id) REFERENCES handoffs(workspace_id, id) ON DELETE CASCADE,
    CHECK (num_nonnulls(version_id, event_key, handoff_id) = 1),
    CHECK ((file_id IS NULL) = (version_id IS NULL)),
    CHECK ((session_id IS NULL) = (event_key IS NULL))
);
CREATE INDEX knowledge_evidence_item_idx ON knowledge_evidence (item_id);

ALTER TABLE knowledge_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE knowledge_evidence ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON knowledge_items, knowledge_evidence FROM PUBLIC, aidrive_app;
GRANT SELECT ON knowledge_items, knowledge_evidence TO aidrive_app;
CREATE POLICY knowledge_items_read ON knowledge_items FOR SELECT USING (app.has_role(workspace_id, 'viewer'));
CREATE POLICY knowledge_evidence_read ON knowledge_evidence FOR SELECT USING (app.has_role(workspace_id, 'viewer'));

-- Evidence JSON: [{"file_id","version_id","quote"} | {"session_id","event_key","quote"} | {"handoff_id","quote"}]
CREATE FUNCTION app.insert_evidence(p_workspace uuid, p_item uuid, p_evidence jsonb) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF jsonb_typeof(coalesce(p_evidence, '[]'::jsonb)) <> 'array' OR jsonb_array_length(coalesce(p_evidence, '[]'::jsonb)) > 10 THEN
        RAISE EXCEPTION 'invalid evidence' USING ERRCODE = 'check_violation';
    END IF;
    -- Session evidence must come from a session of this workspace (the event
    -- foreign key alone does not carry the workspace).
    IF EXISTS (
        SELECT 1 FROM jsonb_array_elements(coalesce(p_evidence, '[]'::jsonb)) e
        WHERE e ? 'session_id' AND NOT EXISTS (
            SELECT 1 FROM public.sessions s WHERE s.id = (e->>'session_id')::uuid AND s.workspace_id = p_workspace)
    ) THEN
        RAISE EXCEPTION 'foreign session evidence' USING ERRCODE = 'foreign_key_violation';
    END IF;
    INSERT INTO public.knowledge_evidence (workspace_id, item_id, file_id, version_id, session_id, event_key, handoff_id, quote)
    SELECT p_workspace, p_item, (e->>'file_id')::uuid, (e->>'version_id')::uuid, (e->>'session_id')::uuid,
           e->>'event_key', (e->>'handoff_id')::uuid, nullif(btrim(e->>'quote'), '')
    FROM jsonb_array_elements(coalesce(p_evidence, '[]'::jsonb)) e;
END
$$;
REVOKE ALL ON FUNCTION app.insert_evidence(uuid, uuid, jsonb) FROM PUBLIC;

-- Editors/owners and files:write agents propose; keystones (project rules
-- every assistant must follow) can only be proposed, never self-approved.
CREATE FUNCTION app.propose_item(
    p_workspace uuid, p_request uuid, p_kind text, p_topic text, p_statement text, p_outcome text,
    p_supersedes uuid, p_evidence jsonb
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
    IF p_request IS NULL THEN RAISE EXCEPTION 'request id required' USING ERRCODE = 'check_violation'; END IF;
    IF p_supersedes IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_supersedes AND k.status = 'approved'
    ) THEN
        RAISE EXCEPTION 'only an approved item can be corrected' USING ERRCODE = 'check_violation';
    END IF;
    PERFORM pg_advisory_xact_lock(hashtextextended('knowledge' || p_workspace::text || app.current_user_id()::text || p_request::text, 0));
    SELECT * INTO v_prior FROM public.knowledge_items k
    WHERE k.workspace_id = p_workspace AND k.actor_id = app.current_user_id() AND k.request_id = p_request;
    IF FOUND THEN
        IF v_prior.kind IS DISTINCT FROM p_kind OR v_prior.statement IS DISTINCT FROM btrim(p_statement)
           OR v_prior.topic IS DISTINCT FROM nullif(lower(btrim(p_topic)), '') OR v_prior.agent_id IS DISTINCT FROM v_agent THEN
            RAISE EXCEPTION 'request ID reused with different input' USING ERRCODE = 'AD409';
        END IF;
        RETURN v_prior.id;
    END IF;
    INSERT INTO public.knowledge_items (workspace_id, kind, topic, statement, outcome, supersedes, actor_id, agent_id, request_id)
    VALUES (p_workspace, p_kind, nullif(lower(btrim(p_topic)), ''), btrim(p_statement), p_outcome, p_supersedes,
            app.current_user_id(), v_agent, p_request)
    RETURNING id INTO v_id;
    PERFORM app.insert_evidence(p_workspace, v_id, p_evidence);
    RETURN v_id;
END
$$;
REVOKE ALL ON FUNCTION app.propose_item(uuid, uuid, text, text, text, text, uuid, jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.propose_item(uuid, uuid, text, text, text, text, uuid, jsonb) TO aidrive_app;

CREATE FUNCTION app.require_human_owner(p_workspace uuid) RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL THEN
        RAISE EXCEPTION 'agents cannot decide' USING ERRCODE = 'insufficient_privilege';
    END IF;
    PERFORM 1 FROM public.memberships m
    WHERE m.workspace_id = p_workspace AND m.workspace_id = app.current_workspace_id()
      AND m.user_id = app.current_user_id() AND m.role = 'owner'
    FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'owner required' USING ERRCODE = 'insufficient_privilege'; END IF;
END
$$;
REVOKE ALL ON FUNCTION app.require_human_owner(uuid) FROM PUBLIC;

-- Approve or reject one proposed item. `p_statement` (optional) is the
-- owner's edit: the edited text is approved as a new revision and the
-- proposal is marked superseded by it, so the original wording stays visible.
CREATE FUNCTION app.decide_item(p_workspace uuid, p_item uuid, p_decision text, p_note text, p_statement text)
RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_item public.knowledge_items%ROWTYPE;
    v_result uuid := p_item;
BEGIN
    PERFORM app.require_human_owner(p_workspace);
    IF p_decision NOT IN ('approved', 'rejected') THEN
        RAISE EXCEPTION 'invalid decision' USING ERRCODE = 'check_violation';
    END IF;
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    IF v_item.status <> 'proposed' THEN RAISE EXCEPTION 'already decided' USING ERRCODE = 'AD409'; END IF;
    IF p_decision = 'rejected' THEN
        UPDATE public.knowledge_items SET status = 'rejected', decided_by = app.current_user_id(),
            decided_at = clock_timestamp(), decision_note = nullif(btrim(p_note), '')
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
    IF nullif(btrim(p_statement), '') IS NOT NULL AND btrim(p_statement) <> v_item.statement THEN
        INSERT INTO public.knowledge_items (workspace_id, kind, topic, statement, outcome, supersedes, actor_id, request_id,
                                            status, decided_by, decided_at, decision_note)
        VALUES (p_workspace, v_item.kind, v_item.topic, btrim(p_statement), v_item.outcome, v_item.supersedes,
                app.current_user_id(), gen_random_uuid(), 'approved', app.current_user_id(), clock_timestamp(),
                coalesce(nullif(btrim(p_note), ''), 'Edited by owner'))
        RETURNING id INTO v_result;
        INSERT INTO public.knowledge_evidence (workspace_id, item_id, file_id, version_id, session_id, event_key, handoff_id, quote)
        SELECT workspace_id, v_result, file_id, version_id, session_id, event_key, handoff_id, quote
        FROM public.knowledge_evidence WHERE item_id = p_item;
        UPDATE public.knowledge_items SET status = 'superseded', decided_by = app.current_user_id(), decided_at = clock_timestamp(),
            decision_note = 'Approved with edits as ' || v_result::text
        WHERE id = p_item;
    ELSE
        UPDATE public.knowledge_items SET status = 'approved', decided_by = app.current_user_id(),
            decided_at = clock_timestamp(), decision_note = nullif(btrim(p_note), '')
        WHERE id = p_item;
    END IF;
    IF v_item.supersedes IS NOT NULL THEN
        UPDATE public.knowledge_items SET status = 'superseded'
        WHERE workspace_id = p_workspace AND id = v_item.supersedes AND status = 'approved';
    END IF;
    RETURN v_result;
END
$$;
REVOKE ALL ON FUNCTION app.decide_item(uuid, uuid, text, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.decide_item(uuid, uuid, text, text, text) TO aidrive_app;

-- Approving a handoff turns each of its decisions into an approved decision
-- item, with the handoff (and through it the cited file versions) as evidence.
CREATE FUNCTION app.items_from_approved_handoff() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    d text;
    v_item uuid;
BEGIN
    IF NEW.status = 'approved' AND OLD.status = 'proposed' THEN
        FOREACH d IN ARRAY NEW.decisions LOOP
            INSERT INTO public.knowledge_items (workspace_id, kind, statement, actor_id, agent_id, request_id,
                                                status, decided_by, decided_at, decision_note)
            VALUES (NEW.workspace_id, 'decision', d, NEW.actor_id, NEW.agent_id, gen_random_uuid(),
                    'approved', NEW.decided_by, NEW.decided_at, 'From approved handoff')
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
REVOKE ALL ON FUNCTION app.items_from_approved_handoff() FROM PUBLIC;
CREATE TRIGGER handoffs_create_items AFTER UPDATE OF status ON handoffs
FOR EACH ROW EXECUTE FUNCTION app.items_from_approved_handoff();

-- One-time backfill: decisions of handoffs that were already approved before
-- this migration become approved items too (superseded handoffs are history
-- and are not backfilled).
WITH approved AS (
    SELECT h.*, d.decision, d.ord
    FROM public.handoffs h, unnest(h.decisions) WITH ORDINALITY AS d(decision, ord)
    WHERE h.status = 'approved'
), inserted AS (
    INSERT INTO public.knowledge_items (workspace_id, kind, statement, actor_id, agent_id, request_id,
                                        status, decided_by, decided_at, decision_note)
    SELECT workspace_id, 'decision', decision, actor_id, agent_id, gen_random_uuid(),
           'approved', decided_by, decided_at, 'From approved handoff (backfill)'
    FROM approved
    RETURNING id, workspace_id, statement, decided_at
)
INSERT INTO public.knowledge_evidence (workspace_id, item_id, handoff_id, quote)
SELECT i.workspace_id, i.id, a.id, left(a.summary, 500)
FROM inserted i JOIN approved a ON a.workspace_id = i.workspace_id AND a.decision = i.statement AND a.decided_at = i.decided_at;

INSERT INTO public.knowledge_evidence (workspace_id, item_id, file_id, version_id)
SELECT e.workspace_id, e.item_id, s.file_id, s.version_id
FROM public.knowledge_evidence e JOIN public.handoff_sources s ON s.handoff_id = e.handoff_id
JOIN public.knowledge_items k ON k.id = e.item_id AND k.decision_note = 'From approved handoff (backfill)';
