-- Run as the superuser on a copy migrated through 0017, seeded with three
-- keys, then migrated to head (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
DO $$
DECLARE r record;
BEGIN
    FOR r IN SELECT name, created_at, expires_at, revoked_at FROM agent_grants WHERE name LIKE 'B-%' LOOP
        IF r.name = 'B-valid' AND r.expires_at - r.created_at <> interval '30 days' THEN
            RAISE EXCEPTION 'FAIL L02: a valid key was not extended to 30 days (%)', r.expires_at - r.created_at;
        ELSIF r.name = 'B-expired' AND r.expires_at > clock_timestamp() THEN
            RAISE EXCEPTION 'FAIL L02: an expired key came back to life';
        ELSIF r.name = 'B-revoked' AND r.expires_at - r.created_at <> interval '7 days' THEN
            RAISE EXCEPTION 'FAIL L02: a revoked key was changed';
        END IF;
    END LOOP;
    IF (SELECT count(*) FROM agent_grants WHERE name LIKE 'B-%') <> 3 THEN
        RAISE EXCEPTION 'FAIL L02: seed missing';
    END IF;
    RAISE NOTICE 'PASS L02 upgrade: valid keys get 30 days from creation; expired and revoked keys are unchanged';
END $$;
