-- Pilot review (2026-11): a constraint shows in every assistant's brief under
-- "Rules (must follow)", like a keystone, so an assistant could plant a lasting
-- instruction for all the others by proposing one in a space that reviews by
-- exception. Constraints now wait for the owner too. Already accepted ones are
-- left as they are; the owner can undo them under Review.
CREATE OR REPLACE FUNCTION app.auto_accept_item(p_workspace uuid, p_item uuid) RETURNS boolean
LANGUAGE plpgsql VOLATILE SECURITY DEFINER SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    v_item public.knowledge_items%ROWTYPE;
    v_now timestamptz;
BEGIN
    IF app.review_mode(p_workspace) <> 'exceptions' THEN RETURN false; END IF;
    SELECT * INTO v_item FROM public.knowledge_items k WHERE k.workspace_id = p_workspace AND k.id = p_item FOR UPDATE;
    IF NOT FOUND OR v_item.status <> 'proposed' OR v_item.kind IN ('keystone', 'constraint') OR v_item.supersedes IS NOT NULL
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
