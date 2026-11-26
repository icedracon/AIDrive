-- Run as aidrive_app on a database migrated through 0003 (scripts/db-check-private.sh).
\set ON_ERROR_STOP 1
\set QUIET 1
SET client_min_messages = notice;
\set hash '$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNoaGFzaGhhc2hoYXNoaGFzaGhhc2g'

DO $$ BEGIN
    PERFORM count(*) FROM auth_credentials;
    RAISE EXCEPTION 'FAIL A01: app role read credentials';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS A01 credentials table not readable by app role';
END $$;
DO $$ BEGIN
    PERFORM count(*) FROM auth_sessions;
    RAISE EXCEPTION 'FAIL A02: app role read sessions';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS A02 sessions table not readable by app role';
END $$;
DO $$ BEGIN
    INSERT INTO auth_sessions (user_id, token_hash, expires_at)
    VALUES (gen_random_uuid(), decode(repeat('00', 32), 'hex'), now() + interval '1 year');
    RAISE EXCEPTION 'FAIL A03: app role inserted a session directly';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS A03 direct session insert denied';
END $$;

SELECT app.register_user('  Owner@Example.TEST ', :'hash') AS owner_id \gset
SELECT set_config('test.owner_id', :'owner_id', false) \gset
DO $$ BEGIN
    IF (SELECT user_id FROM app.login_lookup('owner@example.test')) IS DISTINCT FROM current_setting('test.owner_id')::uuid THEN
        RAISE EXCEPTION 'FAIL A04: normalized email lookup';
    END IF;
    IF EXISTS (SELECT 1 FROM app.login_lookup('nobody@example.test')) THEN
        RAISE EXCEPTION 'FAIL A04: unknown email returned a row';
    END IF;
    RAISE NOTICE 'PASS A04 email normalized; unknown email returns no row';
END $$;
DO $$ BEGIN
    PERFORM app.register_user('OWNER@example.test', '$argon2id$v=19$m=19456,t=2,p=1$eA$eA');
    RAISE EXCEPTION 'FAIL A05: duplicate email accepted';
EXCEPTION WHEN unique_violation THEN RAISE NOTICE 'PASS A05 case-insensitive duplicate email rejected';
END $$;
DO $$ BEGIN
    PERFORM app.register_user('weak@example.test', '$2b$10$bcryptbcryptbcryptbcrypt');
    RAISE EXCEPTION 'FAIL A06: non-argon2id hash accepted';
EXCEPTION WHEN check_violation THEN RAISE NOTICE 'PASS A06 non-argon2id hash rejected';
END $$;
DO $$ BEGIN
    PERFORM app.create_session(gen_random_uuid(), decode(repeat('01', 32), 'hex'));
    RAISE EXCEPTION 'FAIL A07: session for unknown user';
EXCEPTION WHEN insufficient_privilege THEN RAISE NOTICE 'PASS A07 session for unknown user denied';
END $$;

DO $$ DECLARE owner uuid := current_setting('test.owner_id')::uuid; BEGIN
    PERFORM app.create_session(owner, decode(repeat('aa', 32), 'hex'));
    IF app.resolve_session(decode(repeat('aa', 32), 'hex')) IS DISTINCT FROM owner THEN
        RAISE EXCEPTION 'FAIL A08: fresh session did not resolve';
    END IF;
    IF app.resolve_session(decode(repeat('ac', 32), 'hex')) IS NOT NULL THEN
        RAISE EXCEPTION 'FAIL A08: unknown token resolved';
    END IF;
    IF NOT app.revoke_session(decode(repeat('aa', 32), 'hex'))
       OR app.resolve_session(decode(repeat('aa', 32), 'hex')) IS NOT NULL
       OR app.revoke_session(decode(repeat('aa', 32), 'hex')) THEN
        RAISE EXCEPTION 'FAIL A08: revocation';
    END IF;
    RAISE NOTICE 'PASS A08 session resolves, unknown token does not, revocation is final';
END $$;

-- Left for the runner to age as superuser: idle and absolute expiry.
SELECT app.create_session(:'owner_id', decode(repeat('bb', 32), 'hex')) IS NOT NULL AS idle_created \gset
SELECT app.create_session(:'owner_id', decode(repeat('cc', 32), 'hex')) IS NOT NULL AS absolute_created \gset
\echo 'AUTH APP-ROLE CHECKS PASSED'
