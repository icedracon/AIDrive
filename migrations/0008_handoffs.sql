-- Handoffs: what an AI (or person) did, the decisions it proposes, the next
-- step, and the exact file versions it relied on. Content is immutable.
-- Only a human owner approves; approval is refused if any source changed.
-- Staleness is computed at read time from current file heads, so there is
-- no background job and no window in which changed evidence looks current.

CREATE TABLE handoffs (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    actor_id uuid NOT NULL REFERENCES users(id),
    agent_id uuid REFERENCES agent_grants(id),
    request_id uuid NOT NULL,
    summary text NOT NULL CHECK (length(btrim(summary)) BETWEEN 1 AND 4000),
    decisions text[] NOT NULL DEFAULT '{}' CHECK (cardinality(decisions) <= 20),
    next_step text NOT NULL CHECK (length(btrim(next_step)) BETWEEN 1 AND 2000),
    status text NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed', 'approved', 'rejected', 'superseded')),
    decided_by uuid REFERENCES users(id),
    decided_at timestamptz,
    decision_note text CHECK (length(decision_note) <= 2000),
    created_at timestamptz NOT NULL DEFAULT clock_timestamp(),
    UNIQUE (workspace_id, id),
    UNIQUE (workspace_id, actor_id, request_id),
    CHECK ((status = 'proposed') = (decided_by IS NULL AND decided_at IS NULL))
);

CREATE INDEX handoffs_workspace_idx ON handoffs (workspace_id, created_at DESC, id);

CREATE TABLE handoff_sources (
    workspace_id uuid NOT NULL,
    handoff_id uuid NOT NULL,
    file_id uuid NOT NULL,
    version_id uuid NOT NULL,
    PRIMARY KEY (handoff_id, file_id),
    FOREIGN KEY (workspace_id, handoff_id) REFERENCES handoffs(workspace_id, id) ON DELETE CASCADE,
    FOREIGN KEY (workspace_id, file_id, version_id) REFERENCES file_versions(workspace_id, file_id, id) ON DELETE CASCADE
);

ALTER TABLE handoffs ENABLE ROW LEVEL SECURITY;
ALTER TABLE handoff_sources ENABLE ROW LEVEL SECURITY;
REVOKE ALL ON handoffs, handoff_sources FROM PUBLIC, aidrive_app;
GRANT SELECT ON handoffs, handoff_sources TO aidrive_app;
CREATE POLICY handoffs_read ON handoffs FOR SELECT USING (app.has_role(workspace_id, 'viewer'));
CREATE POLICY handoff_sources_read ON handoff_sources FOR SELECT USING (app.has_role(workspace_id, 'viewer'));

CREATE FUNCTION app.record_handoff(
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
    RETURN v_id;
END
$$;
REVOKE ALL ON FUNCTION app.record_handoff(uuid, uuid, text, text[], text, jsonb) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.record_handoff(uuid, uuid, text, text[], text, jsonb) TO aidrive_app;

-- Owner decision on one proposed handoff. Agents can never decide, even
-- with the owner's write key; approval locks the cited files and requires
-- each to still be at the cited version.
CREATE FUNCTION app.decide_handoff(p_workspace uuid, p_handoff uuid, p_decision text, p_note text)
RETURNS void
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_status text;
BEGIN
    IF nullif(current_setting('app.agent_id', true), '') IS NOT NULL THEN
        RAISE EXCEPTION 'agents cannot decide handoffs' USING ERRCODE = 'insufficient_privilege';
    END IF;
    PERFORM 1 FROM public.memberships m
    WHERE m.workspace_id = p_workspace AND m.workspace_id = app.current_workspace_id()
      AND m.user_id = app.current_user_id() AND m.role = 'owner'
    FOR SHARE;
    IF NOT FOUND THEN RAISE EXCEPTION 'owner required' USING ERRCODE = 'insufficient_privilege'; END IF;
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
        UPDATE public.handoffs SET status = 'superseded'
        WHERE workspace_id = p_workspace AND status = 'approved';
    END IF;
    UPDATE public.handoffs
    SET status = p_decision, decided_by = app.current_user_id(), decided_at = clock_timestamp(),
        decision_note = nullif(btrim(p_note), '')
    WHERE workspace_id = p_workspace AND id = p_handoff;
END
$$;
REVOKE ALL ON FUNCTION app.decide_handoff(uuid, uuid, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.decide_handoff(uuid, uuid, text, text) TO aidrive_app;
