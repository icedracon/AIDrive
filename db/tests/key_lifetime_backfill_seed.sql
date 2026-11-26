-- Run as aidrive_app on a database migrated through 0017.
\set ON_ERROR_STOP 1
\set QUIET 1
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'
SELECT app.register_user('b-owner@example.test', :'hash') AS owner_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Key backfill') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
SELECT app.issue_agent_grant(:'ws', gen_random_uuid(), 'B-valid', decode(repeat('b1', 32), 'hex'), 'files:write');
SELECT app.issue_agent_grant(:'ws', gen_random_uuid(), 'B-expired', decode(repeat('b2', 32), 'hex'), 'files:write');
SELECT app.issue_agent_grant(:'ws', gen_random_uuid(), 'B-revoked', decode(repeat('b3', 32), 'hex'), 'files:write');
COMMIT;
