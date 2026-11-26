-- Assistant keys lived 7 days, so a person who came back to a project on day 7
-- or later found every tool refusing ("invalid key"), which also broke the
-- pilot's day-7 return measurement (docs/pilot/TRACKER.md, blocker 1).
-- Keys now live 30 days. Keys that are still valid get the same 30 days from
-- their creation; expired and revoked keys stay as they are. Revocation, issuer
-- demotion and workspace checks are unchanged, so the owner can still end a key
-- at any time.

ALTER TABLE agent_grants ALTER COLUMN expires_at SET DEFAULT statement_timestamp() + interval '30 days';

UPDATE agent_grants
SET expires_at = greatest(expires_at, created_at + interval '30 days')
WHERE revoked_at IS NULL AND expires_at > clock_timestamp();
