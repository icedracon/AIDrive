# Database

## Roles

| Role | Login | Purpose |
| --- | --- | --- |
| `aidrive_owner` | no | Owns the database, schema, tables and `app.*` functions. Migrations run as this role via `SET ROLE`. |
| `aidrive_app` | yes | The only role the API may use. Not superuser, no `BYPASSRLS`, owns nothing, cannot insert workspaces directly. |

`db/bootstrap.sql` creates roles and the database using administrator credentials. It sets the application password only when creating the role; repeated bootstrap never rotates it. `scripts/db-setup.sh <db> [--adopt-initial]` bootstraps and invokes the separate `aidrive-db` migration binary as `aidrive_owner`. Runtime credentials cannot migrate. The API still has no database connection.

## Authentication tables (migration 3)

`auth_credentials` and `auth_sessions` have RLS enabled, no policies and no grants to `aidrive_app`. The API reaches them only through `app.register_user`, `app.login_lookup`, `app.create_session`, `app.resolve_session` and `app.revoke_session`. Session lifetimes are enforced in those functions. See ADR 0002.

## Workspace access (migration 4)

`app.my_workspaces()` lists the caller's memberships across workspaces (the table policies only expose the selected workspace). A `BEFORE UPDATE OR DELETE` trigger on `memberships` refuses to remove or demote the last owner; it locks the workspace row so concurrent demotions serialise, and it allows the cascade when the whole workspace is deleted. The API sets `app.user_id`/`app.workspace_id` only from a validated session, transaction-locally.

## Authorization model

- The API authenticates the user, then sets transaction-local `app.user_id` and `app.workspace_id` (`set_config(..., true)`). These values are request scoping, not authentication: whoever controls the connection controls them, so `aidrive_app` credentials must never reach clients.
- `app.has_role(workspace, minimum)` returns true only for the session's selected workspace and a membership at or above `viewer < editor < owner`. It is `SECURITY DEFINER` with a pinned `search_path` and `EXECUTE` revoked from `PUBLIC`.
- Per-command policies: viewers read; editors write files; owners manage memberships, the workspace, and approved memories/evidence. Inserts must attribute `created_by` / `approved_by` to the acting user.
- `app.create_workspace(name)` creates the workspace and the creator's owner membership atomically. It is the only creation path, which removes self-enrolment.
- RLS is enabled, not forced. The owner bypasses it so the helper functions can read `memberships` without recursive policy evaluation; `aidrive_app` is always subject to it.
- Hidden rows in UPDATE/DELETE are silently skipped (0 rows), not errors; callers must check affected-row counts.

The `vector` extension was removed from the initial migration because nothing uses it yet and it needs elevated privileges. Add it with the retrieval milestone.

## Decision record — 2026-11-17

Previous draft defects observed on a fresh PostgreSQL 17.12 database with a restricted role:

- A user could `INSERT` themselves into any workspace as `owner` (observed: insert accepted).
- `files`/`memories`/`memory_sources` policies compared `m.workspace_id = workspace_id` inside a subquery on `memberships`; the unqualified column bound to `memberships`, making the condition a tautology. Reads stayed hidden only because the `memberships` policy happened to filter the subquery.
- No read/write role separation; Compose connected with the superuser, which bypasses RLS entirely.

Sources (read 2026-11-17): PostgreSQL 17 [Row Security Policies](https://www.postgresql.org/docs/17/ddl-rowsecurity.html) (superuser/`BYPASSRLS` always bypass; owners bypass unless forced; policy sub-SELECTs can race and a security definer function is a suggested mitigation), [CREATE POLICY](https://www.postgresql.org/docs/17/sql-createpolicy.html) (WITH CHECK errors vs silent USING filtering; permissive policies OR together), [CREATE FUNCTION — Writing SECURITY DEFINER Functions Safely](https://www.postgresql.org/docs/17/sql-createfunction.html) (trusted `search_path`, revoke `PUBLIC` execute).

Alternatives considered: (a) inline membership sub-SELECTs with qualified columns — simpler but each policy repeats the role logic and inherits the documented race; (b) app-only authorization with no RLS — one bug leaks across tenants; (c) chosen: one definer helper used by all policies plus a creation function. Tradeoff: the definer functions are privileged code and must stay small and reviewed; owner-bypass means tests must never run as the owner.

Not yet covered: last-owner removal protection, membership changes by an owner to other owners, `users` visibility of co-members, email-existence disclosure through the unique constraint, foreign-key existence probing (FK checks bypass RLS), and connection-pool reset of settings (transaction-local settings mitigate this).

## Foundation verification — 2026-11-18

Supported local path: Linux/WSL, PostgreSQL 17 server and client tools, a non-root OS user and the Rust toolchain. No running development database is required for tests.

```bash
cargo build --locked -p aidrive-db
bash scripts/db-check.sh
```

If using a separate Cargo target directory, set `AIDRIVE_MIGRATOR` to the absolute compiled executable path. Set `AIDRIVE_PG_BIN` when `pg_config --bindir` does not identify the installed server/client binaries (on this machine: `/usr/lib/postgresql/17/bin`).

The checker creates a new cluster under a private `/tmp/aidrive-pg.*` directory, uses SCRAM authentication over its private Unix socket, disables TCP, replaces ambient connection settings, and cleans up only that instance. Explicit external test-target overrides are refused. Application-policy checks authenticate as `aidrive_app`; administration uses a separate connection. The suite covers:

- Original 20 isolation checks against the old schema and the upgraded fresh schema.
- Reproduction of creator spoofing before migration 2; rejection afterward. Creation time and identity are protected, valid editor renames work, and timestamps are server-set.
- Fresh migrations; repeated no-op; password preservation; denied runtime-role migration.
- Verified legacy-schema adoption with rows preserved; refusal when schema grants differ.
- Applied checksum mismatch; transactional rollback of a failing migration; two concurrent runners applying one migration once.
- Refusal of a caller-supplied external test database.

The original isolation tests exercise policy behavior, not end-to-end user authentication. Per-edit actor/version history remains G1 work; preserving `created_by` alone is not an activity system.

## Agent grants (migration 0005)

`agent_grants` is private to `aidrive_owner`, with RLS enabled and no direct runtime table privileges. Metadata includes a client-generated request UUID, workspace, issuing user, display name, fixed `workspace:read` scope, SHA-256 credential digest, creation/expiry timestamps and a revocation tombstone. Runtime access is limited to `app.issue_agent_grant`, `app.list_agent_grants`, `app.revoke_agent_grant` and `app.read_agent_workspace`. The internal owner-lock helper is not executable by the runtime role or PUBLIC.

Management checks the authenticated user's transaction-local workspace and current owner membership. Agent reads take only a token digest plus target workspace and do not impersonate a human. The membership trigger permanently revokes grants when their issuing owner is demoted, removed or moved. `FOR SHARE` locks serialize read authorization against revocation and membership changes; expiry is checked again after acquiring the grant lock. Existing grants cannot be expanded into file or memory access.

`scripts/api-check.sh` exercises the real router and restricted database role, with a separate private fixture administrator only for aging expiry, changing memberships and holding concurrency locks. It verifies cross-workspace denial, viewer/editor denial, cookie/bearer separation, duplicate request handling, hash-only storage, direct-table denial, revocation retries, expiry, demotion/re-promotion, removal/rejoining, a read waiting behind revocation, expiry during a lock wait and connection-pool context cleanup. These fixtures never target the development database.

## Text versions and agent attribution (migrations 0006–0007)

`files.current_version_id` points to an immutable `file_versions` row containing the UTF-8 bytes, hash, revision, parent, optional restored source and actor. `file_actions` records each successful create/update/restore and the request fingerprint in the same transaction. Composite foreign keys keep versions and restore sources within the same file/workspace. Runtime SELECT uses RLS; direct mutation of versions/actions and direct head changes are denied. Only the narrow conditional-write function appends versions and moves the head. Migration 0007 adds agent attribution and validates the grant before establishing the scoped transaction.

The private API suite tests exact Armenian/CRLF bytes, safe downloads, membership and workspace isolation, validation, request replay/conflict, two overlapping writers, restore-as-new-version, direct tamper denial and injected failure at action insertion after bytes/head changes. Failure leaves all three unchanged; retry succeeds once. The suite also restarts its private PostgreSQL instance and uses a fresh API test process to verify the session, content, hash, head and attribution. No development database is used by this suite. Legacy preservation compares every pre-existing column and separately asserts the added head column is NULL; legacy metadata is not a byte migration.

## Migration lifecycle

SQLx is pinned to 0.8.6, compatible with the declared Rust 1.88 baseline; dependency resolution selected compatible versions. Local builds used the installed WSL compiler, not Rust 1.88 itself. `_sqlx_migrations` records versions/checksums/success. The tool holds SQLx's PostgreSQL advisory lock across adoption, validation and application; it disables only the migrator's nested locking, not the outer lock. Migration 1 is unchanged; migration 2 adds provenance protection. Git preserves migration file bytes so line-ending conversion cannot invalidate recorded checksums.

Use explicit `PGHOST`, `PGPORT`, `PGUSER`, `PGPASSWORD` and `PGDATABASE` settings for administrator/migration access. Service-file, host-address and options overrides are rejected by the migrator to keep SQLx and pg_dump on the same connection target. Set `AIDRIVE_APP_PASSWORD` for initial bootstrap; changing this variable on an existing cluster does not change its existing role password. Credential rotation must be a separate deliberate administrative operation.

```bash
# With administrator connection environment already configured:
export AIDRIVE_APP_PASSWORD='<new local app password>'
bash scripts/db-setup.sh aidrive
```

For the pre-ledger database, normal migration refuses to proceed. Take a backup, stop application writers AND other administrative schema changes, then explicitly request verified adoption:

```bash
bash scripts/db-setup.sh aidrive --adopt-initial
```

Adoption creates an isolated reference cluster from the exact initial migration, compares schema-only dumps (including functions, policies, owners and grants), and rejects any difference. PostgreSQL dump wrapper tokens/version headers are ignored; SQL content is retained. Target tables are locked against writes while an exported snapshot is dumped. Only a matching schema receives a baseline ledger entry; pending migrations then run normally. Use the same PostgreSQL major version/tools for reference and target. This is a maintenance operation: the advisory lock coordinates migration tools but cannot prevent an independent administrator from changing functions/grants concurrently. Do not bypass a mismatch by manually populating the ledger. No real development database has been adopted or upgraded by this implementation session.

Compose initialization now bootstraps only roles/database; run the migration binary from the host afterward using explicit administrator settings. The container does not contain Rust. Compose and remote CI remain unverified. The CI database job builds the same utility and runs the private-cluster suite as the runner user.

## Review follow-up

F01's creator-rewrite defect is fixed by migration 2, including server creation/modification timestamps. F02's shared-cluster test/password behavior is replaced by a private test cluster and non-rotating bootstrap. F03's migration replay is replaced by tracked, checksum-validated, locked SQLx migrations with verified legacy adoption. Full G0 is still open: live web/API/database readiness (F04) and the remaining baseline/CI evidence (F05) are separate work.

Research: [SQLx 0.8.6 Migrator](https://docs.rs/sqlx/0.8.6/sqlx/migrate/struct.Migrator.html) supplies migration validation and locking; [PostgreSQL initdb](https://www.postgresql.org/docs/17/app-initdb.html) supports a separately owned cluster; [PostgreSQL privileges](https://www.postgresql.org/docs/17/ddl-priv.html) distinguish row/table authority from field immutability; [official Ubuntu installation guidance](https://www.postgresql.org/download/linux/ubuntu/) informs CI package setup. Sources checked during this work. No fresh-setup/test path deletes an existing development database.
