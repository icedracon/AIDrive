-- Run once per cluster as a superuser:
--   AIDRIVE_APP_PASSWORD=... psql -v db_name=aidrive -f db/bootstrap.sql
-- aidrive_owner owns the schema and runs migrations; it cannot log in.
-- aidrive_app is the only role the API uses and is always subject to RLS.
\set ON_ERROR_STOP 1
\getenv app_password AIDRIVE_APP_PASSWORD

SELECT 'CREATE ROLE aidrive_owner NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS'
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'aidrive_owner') \gexec

SELECT format('CREATE ROLE aidrive_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOINHERIT PASSWORD %L', :'app_password')
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'aidrive_app') \gexec

DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'aidrive_app'
        AND (rolsuper OR rolbypassrls OR rolcreatedb OR rolcreaterole))
        OR pg_has_role('aidrive_app', 'aidrive_owner', 'MEMBER') THEN
        RAISE EXCEPTION 'existing aidrive_app role is privileged; bootstrap refused';
    END IF;
END $$;

SELECT format('CREATE DATABASE %I OWNER aidrive_owner', :'db_name')
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = :'db_name') \gexec

SELECT format('REVOKE ALL ON DATABASE %I FROM PUBLIC', :'db_name') \gexec
SELECT format('GRANT CONNECT ON DATABASE %I TO aidrive_app', :'db_name') \gexec
