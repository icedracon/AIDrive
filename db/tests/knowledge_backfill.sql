-- Run as the cluster superuser after knowledge_backfill_seed.sql (on 0012)
-- and a migration to head. Verifies 0013's validity backfill row by row.
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
DO $$
DECLARE
    x knowledge_items; p knowledge_items; r knowledge_items; y knowledge_items; z knowledge_items;
BEGIN
    SELECT * INTO STRICT x FROM knowledge_items WHERE statement = 'X flat price';
    SELECT * INTO STRICT p FROM knowledge_items WHERE statement = 'P flat price with discount';
    SELECT * INTO STRICT r FROM knowledge_items WHERE statement = 'R flat price with 20% discount';
    SELECT * INTO STRICT y FROM knowledge_items WHERE statement = 'Y five competitors';
    SELECT * INTO STRICT z FROM knowledge_items WHERE statement = 'Z six competitors';
    IF x.status IS DISTINCT FROM 'superseded' OR x.valid_from IS DISTINCT FROM x.decided_at
       OR x.superseded_by IS DISTINCT FROM r.id OR x.invalid_at IS DISTINCT FROM r.decided_at
       OR x.invalidated_by IS DISTINCT FROM r.decided_by THEN
        RAISE EXCEPTION 'FAIL B01: corrected item X % % % %', x.status, x.valid_from, x.superseded_by, x.invalid_at;
    END IF;
    IF p.valid_from IS NOT NULL OR p.invalid_at IS NOT NULL OR p.superseded_by IS DISTINCT FROM r.id THEN
        RAISE EXCEPTION 'FAIL B02: edited proposal P';
    END IF;
    IF r.status IS DISTINCT FROM 'approved' OR r.valid_from IS DISTINCT FROM r.decided_at OR r.invalid_at IS NOT NULL
       OR z.valid_from IS DISTINCT FROM z.decided_at OR z.invalid_at IS NOT NULL THEN
        RAISE EXCEPTION 'FAIL B03: current items';
    END IF;
    IF y.superseded_by IS DISTINCT FROM z.id OR y.valid_from IS DISTINCT FROM y.decided_at OR y.invalid_at IS DISTINCT FROM z.decided_at THEN
        RAISE EXCEPTION 'FAIL B04: directly corrected item Y';
    END IF;
    IF EXISTS (SELECT 1 FROM knowledge_items WHERE statement IN ('Q pending', 'J rejected')
               AND num_nonnulls(valid_from, invalid_at, superseded_by, invalidated_by) > 0) THEN
        RAISE EXCEPTION 'FAIL B05: undecided items gained validity';
    END IF;
    IF (SELECT visibility FROM sessions WHERE external_id = 'pre-0015') IS DISTINCT FROM 'private'
       OR EXISTS (SELECT 1 FROM knowledge_items WHERE visibility <> 'team') THEN
        RAISE EXCEPTION 'FAIL B02: 0015 visibility defaults for existing rows';
    END IF;
    RAISE NOTICE 'PASS B02 0015: sessions captured earlier become private, existing knowledge stays team';
    RAISE NOTICE 'PASS B01 0013 backfill: corrected items valid until their approved successor, edited proposals point at their revision without ever being valid, current items open-ended, pending and rejected untouched';
END $$;
