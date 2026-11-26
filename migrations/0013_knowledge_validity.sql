-- Knowledge validity and conflicts (docs/research/ai-memory-findings.md, plan M2).
-- Recorded time stays created_at/decided_at. Valid time is the interval in
-- which an item was current approved knowledge: valid_from (approval) to
-- invalid_at (superseded or retired). Nothing is deleted; `as_of` recall
-- reads the interval. A proposal conflicts with approved items of the same
-- kind and topic (attempts are history and never conflict). While it is
-- pending those items are disputed, and approving it needs an explicit owner
-- resolution: replace them or keep both.

ALTER TABLE knowledge_items
    ADD COLUMN valid_from timestamptz,
    ADD COLUMN invalid_at timestamptz,
    ADD COLUMN superseded_by uuid,
    ADD COLUMN invalidated_by uuid REFERENCES users(id),
    ADD COLUMN invalidation_note text CHECK (length(invalidation_note) <= 2000),
    ADD FOREIGN KEY (workspace_id, superseded_by) REFERENCES knowledge_items(workspace_id, id);
ALTER TABLE knowledge_items DROP CONSTRAINT knowledge_items_status_check;
ALTER TABLE knowledge_items ADD CONSTRAINT knowledge_items_status_check
    CHECK (status IN ('proposed', 'approved', 'rejected', 'superseded', 'retired'));

-- Backfill. An item was approved at some point if it is approved now or a
-- later item corrects it (proposals may only correct approved items).
-- Proposals the owner approved with edits were never approved themselves;
-- their decision note names the revision.
UPDATE knowledge_items k SET valid_from = k.decided_at
WHERE k.status = 'approved'
   OR (k.status = 'superseded' AND EXISTS (
        SELECT 1 FROM knowledge_items n WHERE n.workspace_id = k.workspace_id AND n.supersedes = k.id));
UPDATE knowledge_items k SET superseded_by = (
    SELECT n.id FROM knowledge_items n
    WHERE n.workspace_id = k.workspace_id AND n.supersedes = k.id AND n.valid_from IS NOT NULL
    ORDER BY n.decided_at, n.id LIMIT 1)
WHERE k.status = 'superseded' AND k.valid_from IS NOT NULL;
UPDATE knowledge_items k SET invalid_at = greatest(n.decided_at, k.valid_from), invalidated_by = n.decided_by
FROM knowledge_items n
WHERE n.id = k.superseded_by AND k.status = 'superseded' AND k.valid_from IS NOT NULL;
UPDATE knowledge_items k
SET superseded_by = substring(k.decision_note FROM '^Approved with edits as ([0-9a-f-]{36})$')::uuid
WHERE k.status = 'superseded' AND k.valid_from IS NULL;

ALTER TABLE knowledge_items
    ADD CONSTRAINT knowledge_items_current_check CHECK (status <> 'approved' OR (valid_from IS NOT NULL AND invalid_at IS NULL AND superseded_by IS NULL)),
    ADD CONSTRAINT knowledge_items_undecided_check CHECK (status NOT IN ('proposed', 'rejected') OR num_nonnulls(valid_from, invalid_at, superseded_by) = 0),
    ADD CONSTRAINT knowledge_items_superseded_check CHECK (status <> 'superseded' OR superseded_by IS NOT NULL),
    ADD CONSTRAINT knowledge_items_retired_check CHECK (status <> 'retired' OR (invalid_at IS NOT NULL AND superseded_by IS NULL)),
    ADD CONSTRAINT knowledge_items_interval_check CHECK (invalid_at IS NULL OR (valid_from IS NOT NULL AND invalid_at >= valid_from)),
    ADD CONSTRAINT knowledge_items_invalidated_check CHECK ((invalidated_by IS NULL) = (invalid_at IS NULL));
CREATE INDEX knowledge_items_slot_idx ON knowledge_items (workspace_id, kind, topic)
    WHERE topic IS NOT NULL AND status IN ('approved', 'proposed');
CREATE INDEX knowledge_items_valid_idx ON knowledge_items (workspace_id, valid_from) WHERE valid_from IS NOT NULL;

-- Approved items that approving `p_self` would contradict: same kind and
-- topic, excluding the item it explicitly corrects. Invoker rights, so the
-- caller's row-level security applies.
CREATE FUNCTION app.item_conflicts(p_workspace uuid, p_kind text, p_topic text, p_supersedes uuid, p_self uuid)
RETURNS uuid[]
LANGUAGE sql STABLE SET search_path = pg_catalog, pg_temp
AS $$
    SELECT coalesce(array_agg(c.id ORDER BY c.created_at, c.id), '{}')
    FROM public.knowledge_items c
    WHERE c.workspace_id = p_workspace AND c.status = 'approved' AND p_kind <> 'attempt'
      AND c.kind = p_kind AND c.topic = p_topic AND c.id <> p_self AND c.id IS DISTINCT FROM p_supersedes
$$;
REVOKE ALL ON FUNCTION app.item_conflicts(uuid, text, text, uuid, uuid) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.item_conflicts(uuid, text, text, uuid, uuid) TO aidrive_app;

DROP FUNCTION app.decide_item(uuid, uuid, text, text, text);

-- Approve or reject one proposed item. `p_statement` (optional) is the
-- owner's edit: the edited text is approved as a new revision and the
-- proposal is marked superseded by it. `p_resolution` is required when the
-- proposal conflicts with approved items: 'replace' supersedes them,
-- 'keep_both' keeps them current alongside the new item.
CREATE FUNCTION app.decide_item(p_workspace uuid, p_item uuid, p_decision text, p_note text, p_statement text,
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
    PERFORM app.require_human_owner(p_workspace);
    IF p_decision NOT IN ('approved', 'rejected') OR p_resolution NOT IN ('replace', 'keep_both') THEN
        RAISE EXCEPTION 'invalid decision' USING ERRCODE = 'check_violation';
    END IF;
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
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
    -- Lock the slot so a concurrent approval cannot add a conflict unseen.
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
                                            status, decided_by, decided_at, decision_note, valid_from)
        VALUES (p_workspace, v_item.kind, v_item.topic, btrim(p_statement), v_item.outcome, v_item.supersedes,
                app.current_user_id(), gen_random_uuid(), 'approved', app.current_user_id(), v_now,
                coalesce(nullif(btrim(p_note), ''), 'Edited by owner'), v_now)
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
REVOKE ALL ON FUNCTION app.decide_item(uuid, uuid, text, text, text, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.decide_item(uuid, uuid, text, text, text, text) TO aidrive_app;

-- The owner ends an approved item without a replacement. It stays visible
-- as history and in `as_of` recall before this moment.
CREATE FUNCTION app.retire_item(p_workspace uuid, p_item uuid, p_note text) RETURNS uuid
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_status text;
BEGIN
    PERFORM app.require_human_owner(p_workspace);
    SELECT k.status INTO v_status FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND THEN RAISE EXCEPTION 'not found' USING ERRCODE = 'no_data_found'; END IF;
    IF v_status <> 'approved' THEN RAISE EXCEPTION 'only current approved items can be retired' USING ERRCODE = 'AD409'; END IF;
    UPDATE public.knowledge_items
    SET status = 'retired', invalid_at = greatest(clock_timestamp(), valid_from), invalidated_by = app.current_user_id(),
        invalidation_note = nullif(btrim(p_note), '')
    WHERE id = p_item;
    RETURN p_item;
END
$$;
REVOKE ALL ON FUNCTION app.retire_item(uuid, uuid, text) FROM PUBLIC;
GRANT EXECUTE ON FUNCTION app.retire_item(uuid, uuid, text) TO aidrive_app;

-- Handoff decisions become valid at the handoff's approval.
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
                                                status, decided_by, decided_at, decision_note, valid_from)
            VALUES (NEW.workspace_id, 'decision', d, NEW.actor_id, NEW.agent_id, gen_random_uuid(),
                    'approved', NEW.decided_by, NEW.decided_at, 'From approved handoff', NEW.decided_at)
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
