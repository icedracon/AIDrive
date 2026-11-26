\set ON_ERROR_STOP 1
\set QUIET 1
-- Run in an isolated admin fixture; app-role checks remain enabled except explicit historical setup.
SET ROLE aidrive_app;
SELECT app.register_user('chunks-owner@example.test', '$argon2id$synthetic-fixture') AS owner_id \gset
BEGIN;
SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Synthetic chunks') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
SELECT app.upsert_session(:'ws', 'codex', 'synthetic-chunks', 'Synthetic', 'import', NULL, NULL) AS sid \gset
SELECT set_config('test.ws', :'ws', false), set_config('test.sid', :'sid', false) \gset
DO $$
DECLARE
    body text := repeat('Հայերեն English ', 160000) || 'final-boundary';
    hash text;
    old text;
    f jsonb;
    i integer;
    n integer;
BEGIN
    hash := encode(sha256(convert_to(body, 'UTF8')), 'hex');
    old := left(body,3999) || '…';
    -- Pre-migration unknown provenance; an unapproved repair must be rejected.
    PERFORM app.add_session_events(current_setting('test.ws')::uuid,current_setting('test.sid')::uuid,
        jsonb_build_array(jsonb_build_object('key','historical-hash-key','ordinal',0,'role','user','text',old)),false);
    RESET ROLE;
    UPDATE public.session_events SET capture_provenance_known=false WHERE session_id=current_setting('test.sid')::uuid;
    SET ROLE aidrive_app;
    n := (length(body)+3999)/4000;
    FOR i IN 0..n-1 LOOP
        f := jsonb_build_object('key','native-key','ordinal',0,'role','user','text',substring(body FROM i*4000+1 FOR 4000),
            'chunk_index',i,'chunk_count',n,'content_sha256',hash,'legacy_sha256',encode(sha256(convert_to(old,'UTF8')),'hex'));
        IF i < n-1 THEN
            PERFORM app.add_session_events(current_setting('test.ws')::uuid,current_setting('test.sid')::uuid,jsonb_build_array(f),false);
        ELSE
            BEGIN
                PERFORM app.add_session_events(current_setting('test.ws')::uuid,current_setting('test.sid')::uuid,jsonb_build_array(f),true);
                RAISE EXCEPTION 'FAIL unknown provenance overwritten';
            EXCEPTION WHEN check_violation THEN NULL;
            END;
            PERFORM app.authorize_session_recapture(current_setting('test.ws')::uuid,current_setting('test.sid')::uuid,NULL);
            PERFORM app.add_session_events(current_setting('test.ws')::uuid,current_setting('test.sid')::uuid,jsonb_build_array(f),true);
        END IF;
    END LOOP;
    IF (SELECT c.body FROM public.session_events c WHERE session_id=current_setting('test.sid')::uuid) <> body THEN
        RAISE EXCEPTION 'FAIL full body';
    END IF;
    IF NOT EXISTS(SELECT 1 FROM public.session_events WHERE session_id=current_setting('test.sid')::uuid
        AND event_key='historical-hash-key' AND capture_source_key='native-key' AND capture_version=3) THEN
        RAISE EXCEPTION 'FAIL repair identity';
    END IF;
    BEGIN
        PERFORM 1 FROM public.session_event_chunks;
        RAISE EXCEPTION 'FAIL fragments readable';
    EXCEPTION WHEN insufficient_privilege THEN NULL;
    END;
    -- Full replay and a legacy writer cannot shorten or duplicate the canonical event.
    FOR i IN REVERSE n-1..0 LOOP
        f := jsonb_set(f,'{chunk_index}',to_jsonb(i));
        f := jsonb_set(f,'{text}',to_jsonb(substring(body FROM i*4000+1 FOR 4000)));
        PERFORM app.add_session_events(current_setting('test.ws')::uuid,current_setting('test.sid')::uuid,jsonb_build_array(f),i=0);
    END LOOP;
    PERFORM app.add_session_events(current_setting('test.ws')::uuid,current_setting('test.sid')::uuid,
        jsonb_build_array(jsonb_build_object('key','historical-hash-key','ordinal',0,'role','user','text',old)),true);
    IF (SELECT count(*) FROM public.session_events WHERE session_id=current_setting('test.sid')::uuid) <> 1
       OR (SELECT octet_length(c.body) FROM public.session_events c WHERE session_id=current_setting('test.sid')::uuid) <= 2097152 THEN
        RAISE EXCEPTION 'FAIL replay';
    END IF;
    -- A v4 fallback-key namespace upgrade must reuse an already published v3 event.
    f := jsonb_set(f,'{key}',to_jsonb('sg:synthetic-segment:native-key'::text));
    FOR i IN 0..n-1 LOOP
        f := jsonb_set(f,'{chunk_index}',to_jsonb(i));
        f := jsonb_set(f,'{text}',to_jsonb(substring(body FROM i*4000+1 FOR 4000)));
        PERFORM app.add_session_events(current_setting('test.ws')::uuid,current_setting('test.sid')::uuid,jsonb_build_array(f),i=n-1);
    END LOOP;
    IF (SELECT count(*) FROM public.session_events WHERE session_id=current_setting('test.sid')::uuid) <> 1
       OR NOT EXISTS(SELECT 1 FROM public.session_events WHERE session_id=current_setting('test.sid')::uuid
         AND event_key='historical-hash-key' AND capture_source_key='sg:synthetic-segment:native-key') THEN
        RAISE EXCEPTION 'FAIL v3-to-v4 replay duplicated canonical event';
    END IF;
    RAISE NOTICE 'PASS v3-to-v4 fallback-key replay preserves one canonical event';
    -- Incorrect digest never publishes a canonical row.
    BEGIN
        PERFORM app.add_session_events(current_setting('test.ws')::uuid,current_setting('test.sid')::uuid,
          jsonb_build_array(jsonb_build_object('key','bad-hash','ordinal',1,'role','assistant','text','bad',
          'chunk_index',0,'chunk_count',1,'content_sha256',repeat('0',64),'legacy_sha256',repeat('0',64))),true);
        RAISE EXCEPTION 'FAIL bad digest accepted';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    RAISE NOTICE 'PASS >2MiB bilingual reassembly, explicit historical repair, physical key preservation, out-of-order replay, legacy no-shrink, digest rejection';
END $$;
-- A session creator cannot use its session-level identity to rewrite another writer's event.
SELECT app.issue_agent_grant(:'ws', '70000000-0000-4000-8000-0000000000c1', 'Capture A', sha256(convert_to('session-chunks-recapture-agent-A-fixture','UTF8')), 'files:write');
SELECT app.issue_agent_grant(:'ws', '70000000-0000-4000-8000-0000000000c2', 'Capture B', sha256(convert_to('session-chunks-recapture-agent-B-fixture','UTF8')), 'files:write');
SELECT app.authorize_session_recapture(:'ws', :'sid', '70000000-0000-4000-8000-0000000000c1');
SELECT app.add_session_events(:'ws', :'sid', '[{"key":"human-writer","ordinal":6,"role":"assistant","text":"human writer prefix"}]',false);
SELECT set_config('app.agent_id', '70000000-0000-4000-8000-0000000000c2', true) \gset
SELECT app.add_session_events(:'ws', :'sid', '[{"key":"other-writer","ordinal":5,"role":"assistant","text":"other writer prefix"}]',false);
SELECT set_config('app.agent_id', '70000000-0000-4000-8000-0000000000c1', true) \gset
DO $$ DECLARE body text := 'other writer prefix extended'; BEGIN
    BEGIN
        PERFORM app.add_session_events(current_setting('test.ws')::uuid,current_setting('test.sid')::uuid,
          jsonb_build_array(jsonb_build_object('key','other-writer','ordinal',5,'role','assistant','text',body,
            'chunk_index',0,'chunk_count',1,'content_sha256',encode(sha256(convert_to(body,'UTF8')),'hex'),
            'legacy_sha256',encode(sha256(convert_to('other writer prefix','UTF8')),'hex'))),true);
        RAISE EXCEPTION 'FAIL other writer overwritten';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    IF NOT EXISTS(SELECT 1 FROM session_events WHERE event_key='other-writer' AND session_events.body='other writer prefix') THEN
        RAISE EXCEPTION 'FAIL other writer modified';
    END IF;
    BEGIN
        PERFORM app.add_session_events(current_setting('test.ws')::uuid,current_setting('test.sid')::uuid,
          jsonb_build_array(jsonb_build_object('key','human-writer','ordinal',6,'role','assistant','text','human writer prefix extended',
            'chunk_index',0,'chunk_count',1,'content_sha256',encode(sha256(convert_to('human writer prefix extended','UTF8')),'hex'),
            'legacy_sha256',encode(sha256(convert_to('human writer prefix','UTF8')),'hex'))),true);
        RAISE EXCEPTION 'FAIL known human writer overwritten';
    EXCEPTION WHEN check_violation THEN NULL;
    END;
    RAISE NOTICE 'PASS other agent and known human writer protected despite historical recapture grant';
END $$;
COMMIT;
