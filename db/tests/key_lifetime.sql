-- Run as aidrive_app on a database migrated through 0018 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('l-owner@example.test', :'hash') AS owner_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Lifetime') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
SELECT app.issue_agent_grant(:'ws', '70000000-0000-4000-8000-0000000000f1', 'Claude', decode(repeat('7f', 32), 'hex'), 'files:write');
DO $$ BEGIN
    IF (SELECT expires_at - created_at FROM app.list_agent_grants(current_setting('app.workspace_id')::uuid)
        WHERE id = '70000000-0000-4000-8000-0000000000f1') NOT BETWEEN interval '29 days 23 hours' AND interval '30 days 1 minute' THEN
        RAISE EXCEPTION 'FAIL L01: new key lifetime is not 30 days';
    END IF;
    RAISE NOTICE 'PASS L01 new assistant keys live 30 days, so a person returning after a week is not locked out';
END $$;
COMMIT;
