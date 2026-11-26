-- Run as aidrive_app on a database migrated through 0012 only. Creates every
-- knowledge lifecycle shape 0012 can produce; knowledge_backfill.sql then
-- checks how 0013 backfills validity for them.
\set ON_ERROR_STOP 1
\set QUIET 1
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

SELECT app.register_user('b-owner@example.test', :'hash') AS owner_id \gset
BEGIN; SELECT set_config('app.user_id', :'owner_id', true) \gset
SELECT app.create_workspace('Backfill') AS ws \gset
SELECT set_config('app.workspace_id', :'ws', true) \gset
-- X approved, then corrected by P, which the owner approved with edits as R.
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'X flat price', NULL, NULL, '[]') AS x \gset
SELECT app.decide_item(:'ws', :'x', 'approved', NULL, NULL);
SELECT app.propose_item(:'ws', gen_random_uuid(), 'decision', 'pricing', 'P flat price with discount', NULL, :'x', '[]') AS p \gset
SELECT app.decide_item(:'ws', :'p', 'approved', NULL, 'R flat price with 20% discount');
-- Y approved, then corrected directly by Z (still current).
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'market', 'Y five competitors', NULL, NULL, '[]') AS y \gset
SELECT app.decide_item(:'ws', :'y', 'approved', NULL, NULL);
SELECT app.propose_item(:'ws', gen_random_uuid(), 'fact', 'market', 'Z six competitors', NULL, :'y', '[]') AS z \gset
SELECT app.decide_item(:'ws', :'z', 'approved', NULL, NULL);
-- Q pending, J rejected.
SELECT app.propose_item(:'ws', gen_random_uuid(), 'gotcha', NULL, 'Q pending', NULL, NULL, '[]') AS q \gset
SELECT app.propose_item(:'ws', gen_random_uuid(), 'gotcha', NULL, 'J rejected', NULL, NULL, '[]') AS j \gset
SELECT app.decide_item(:'ws', :'j', 'rejected', NULL, NULL);
-- A session captured before visibility existed.
SELECT app.upsert_session(:'ws', 'codex', 'pre-0015', 'old', 'import', NULL, NULL);
COMMIT;
