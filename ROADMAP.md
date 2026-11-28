# AIDrive — delivery roadmap

Baseline: 2026-11-17. Product definition: docs/PRODUCT.md. This is a proposed sequence, not a promise that the entire product is ready in 30 days.

Execution priority: follow the strict G0-G6 gates below. They refine the original M0-M5 milestones and AID-001–010 backlog; earlier local passes do not waive these release checks. The 9/10 rubric is in docs/PRODUCT.md.

## Existing foundation

- [x] Separate repository initialized; no initial commit recorded yet
- [x] API health/readiness handlers and OpenAPI contract
- [x] Web shell with clearly unavailable product actions
- [x] Cargo/npm lockfiles and CI configuration written
- [x] Local MCP initialization and discovery of get_api_health observed
- [x] Validate database startup/migrations on a fresh database — PostgreSQL 17.12 in WSL Ubuntu, `scripts/db-check.sh`, 20/20 isolation checks as restricted `aidrive_app` (2026-11-17). Compose path not run (no Docker).
- [x] Resolve and verify live API/web connectivity — F04, 2026-11-18 (evidence below)
- [ ] Observe CI running on an authorized remote repository

Previous local checks covered Rust formatting, compilation, one health handler test, Clippy, web lint/build and MCP tool discovery. These results do not establish product readiness or database isolation.

## Milestones and dependency order

| Milestone | Work | Exit evidence |
| --- | --- | --- |
| M0: Product and pilot | Complete scenario, six screen flows, five prospective design partners, current-workflow observations | A concrete demo script and recorded user problems; recruitment still pending |
| M1: Trusted Drive | Authentication, restricted database role, workspaces, per-agent permissions, durable text files, versions, conditional writes | Restart recovery, isolation, forbidden-write and stale-version checks pass |
| M2: Conversations and history | Two in-app provider adapters, message persistence, capture status, activity records and usage limits | Both providers work on the same authorized project; interrupted requests remain recoverable |
| M3: Shared understanding | Source-linked memory candidates, approval, retrieval, task state, freshness invalidation and context manifests | AI B resumes AI A's task; corrected/stale/denied facts are handled properly |
| M4: Portability and external tools | Scoped Passport preview/export/import; one tested external MCP client; explicit handoff tools | Round trip preserves selected context; external capture limitations shown honestly |
| M5: Pilot and funding evidence | Matched-task comparison, return usage, paid-pilot conversations, demo and grounded pitch | Metrics recorded; claims match actual capabilities and user evidence |

User research continues during M1-M4. Each milestone is accepted by behavior, not number of files or crates. Review estimates after M1 establishes the development environment and persistence approach. The initial 30-day window is a timebox for the first complete demo and evidence collection, not a commitment to finish every milestone.

## First implementation backlog

1. AID-001: Specify demo fixtures and six screen flows from the approved product direction. Interactive draft: [prototype](../outputs/AIDrive-interactive-prototype.html). Six views and scripted approval, source version, conflict, recovery, revocation and export flows are present; JavaScript state checks passed on 2026-11-17. Browser check on 2026-11-17 over `http://localhost:8765` (Python static server): renders without console errors; approve → Codex continuation clicked through; no horizontal overflow at 375 px on all six views; visible focus ring and keyboard activation observed; nav uses `aria-current`; toast is `aria-live`. Not done: screen-reader pass, full visual review (screenshots intermittently timed out), user validation. This does not implement the backend or complete M0.
2. AID-002: Make local database startup and migration execution reproducible. Exit: fresh schema and a restricted application role; document actual environment requirements. **Local exit met 2026-11-17** (see docs/DATABASE.md): owner/app role split, self-enrolment and policy-tautology defects fixed, mutation-tested isolation suite, CI job written. Open: Compose run, remote CI observation.
3. AID-003: Implement owner identity, project isolation and agent grants. Exit: unauthorized reads, writes, retrieval and grant escalation denied.
4. AID-004: Implement durable file operations and version-aware proposals. Exit: restart recovery, stale-write rejection and restore as a new version.
5. AID-005: Persist conversations with actor/provenance/capture state. Exit: visible history survives restart; duplicate import/request handling demonstrated.
6. AID-006: Add two provider adapters and a common chat interface. Exit: timeout/failure/cost handling and real source access.
7. AID-007: Add memory review with exact evidence and correction/deletion. Exit: unsupported proposals are not presented as approved knowledge.
8. AID-008: Assemble authorized, bounded context with source freshness checks. Exit: a new provider can continue a task and identify conflicts without manual history copying.
9. AID-009: Add Passport round trip and explicit MCP handoff. Exit: scoped export, validated import, revocation and accurate capture coverage.
10. AID-010: Run the pilot and make a continue/change decision against the product thresholds. Exit: observed results, limitations and next scope recorded.

These identifiers describe local planned work; no external issues have been created.

## Founder decisions still open

- First pilot audience within people who regularly switch AI tools.
- Provider pair and whether pilot model usage is operator-funded or uses participant keys.
- Hosted versus local pilot deployment, retention duration and allowed file sizes.
- Price and paid-pilot terms after usage data is available.

Use the proposed small in-app pilot in PRODUCT.md for planning. Do not interpret these defaults as permission to spend money, deploy publicly, invite users or send applications.

## Strict execution plan for the first 9/10 release

All whole gates remain OPEN. Update 2026-11-18: F01's attribution defect and F02/F03 are implemented and verified in isolated PostgreSQL 17.12; G1 will add per-edit versions/events. F04 and the remaining F05 work are pending, so G0 is not complete. Work proceeds G0 -> G1 -> G2 -> G3 -> G4 -> G5 -> G6. User/problem research and evaluation-fixture design can progress alongside engineering; they do not require waiting for G5. Stop dependent integration when a gate fails; continue unrelated authorized work.

Roles: the implementing contributor owns code and test evidence; a separate review pass checks the changed behavior and failure cases; the founder owns audience, commercial and deployment decisions. A review pass need not be another person or agent, but must inspect evidence rather than repeat the implementation's claim. No agent delegation or external issue creation is implied by this plan.

### G0 — Repair and make the foundation reproducible

Maps to AID-002; must precede real user data.

- F01: Protect file creator and creation time from ordinary updates, including editor updates. Record the modifying actor on an immutable version/event. Add a regression using the editor who could previously replace `created_by` with another workspace user's ID. Inserts and updates need distinct tests.
- F02: Separate test database credentials from development credentials. Prefer a dedicated disposable PostgreSQL instance for tests; a separate database in the same cluster alone does not isolate roles. Test runner must refuse non-test targets and must never rotate `aidrive_app` credentials on the development cluster. Confirm the development login remains valid before/after a test run without exposing secrets.
- F03: Introduce a real migration ledger with checksums and locking. Evaluate SQLx with the project's actual Rust toolchain, select a compatible pinned version, and keep migration credentials separate from runtime credentials. Test fresh install, repeat no-op, old-schema upgrade, failed-migration rollback and modified-migration rejection. Existing databases require schema verification before recording a baseline; never blindly mark migrations applied. Do not rewrite an applied migration to hide drift.
- F04: Establish browser -> web -> API -> restricted PostgreSQL connectivity. `/healthz` reports process liveness; `/readyz` fails when required database/schema/storage dependencies are unusable. No production data or secrets in diagnostics.
- F05: Record a clean repository baseline after inspecting files for secrets. Add the new regression checks to CI. Exercise one supported local startup path and one isolated release/test environment. Do not claim Compose or remote CI passed until run and observed; remote push remains separately authorized.

Exit: F01-F05 evidence recorded, current three review defects closed, prior isolation checks preserved, no unexpected modifications to an existing development database. Prior review reproduced F01 with a rolled-back transaction; F02/F03 were code-inspection findings.

### G1 — Authenticated durable Drive

Maps to AID-003/004. One real UI journey through real services, no fake persistence.

- Use a maintained authentication mechanism, with server-side validated identity and secure session lifecycle; do not accept user identity from request-supplied database settings. Choose and document the mechanism before implementation. An explicitly labeled local test identity mode must be impossible to enable accidentally in a hosted release.
- Define workspace as project boundary; introduce human/agent principals, scoped grants and revocation. A grant cannot exceed the granting user's rights. Test forged IDs, wrong workspace, expired/revoked credentials and connection-pool context reuse. Protect the last owner from accidental removal.
- Implement file metadata, immutable content versions, current-head reference, upload validation, read/download, diff/proposal review and restore-as-new-version. Keep the original creator immutable and each edit actor explicit.
- Condition every change on the base version. Two writes from one base yield one winner and a conflict, never silent overwrite. Persist the change and its action event atomically; stage file bytes before publishing metadata and reconcile interrupted staging.
- Provide useful loading, empty, permission-denied, failed-upload and conflict states in the actual web app.

Exit: authenticated user uploads/edits/restores a file, restarts the services, then reads identical content and provenance. Two isolated workspaces and two agent grants pass read/write/denial checks. Inject failure between byte staging and database commit; no acknowledged version may point to missing bytes. All mutations check affected-row counts.

### G2 — Two real AI conversations and trustworthy activity

Maps to AID-005/006. Depends on G1; provider-independent persistence can be built before credentials arrive.

- Persist user messages before provider dispatch; store streaming responses as pending/partial/completed/failed/cancelled with explicit provenance. Record provider/model, run ID, request ID, capture origin, source manifest and usage.
- Add two real adapters using current official provider documentation and compatible pinned clients. Use the same authorization/context service. Provider tool calls produce proposals; only the authorized service can apply a confirmed mutation.
- Separate an agent's claim of success from a confirmed durable action. Record attempted/completed/failed outcomes and file versions, with redacted diagnostics.
- Implement bounded timeouts, cancellation, token/cost budgets and retry handling. An ambiguous provider timeout may have incurred charges: report uncertainty rather than claiming zero cost. Retries must not duplicate messages or apply a tool mutation twice; do not promise exactly-once provider billing.
- Keep captured in-app turns complete; mark external/imported content and known gaps. Provide search over persisted conversations.

Exit: both providers produce actual saved responses from authorized files; history survives restart. Invalid credentials, rate limiting, dropped stream, timeout, retry and mid-run revocation produce correct visible states. No plaintext provider credentials in database rows, browser payloads or ordinary logs.

### G3 — Shared memory and continuation, the product's central proof

Maps to AID-007/008. Depends on G2. Draft evaluation fixtures before tuning.

- Implement proposed/approved/disputed/stale/superseded/rejected revisions, source-version/message references, proposal and approval actors, and user correction. Approvals authorize a specific revision and source snapshot; reject approval if that evidence changed concurrently.
- Start extraction with structured validated output and evidence references; missing or fabricated evidence cannot become approved memory. Model agreement does not replace owner approval.
- Keep source history, accepted knowledge and task context separate. Store observed task outcomes/next actions with provenance. Changed/deleted sources invalidate dependent memory and summaries before the next delivery; background re-indexing must not leave a freshness window.
- Implement bounded retrieval over permitted current sources plus explicitly historical context. Start with exact filters and PostgreSQL text search; compare a hybrid embedding approach on the development set before adding it. Armenian/English retrieval must be measured rather than assumed.
- Persist context manifests with source IDs/versions, grant revision, token budget, omissions and delivery state. Recheck permission and freshness at dispatch; recheck any later tool mutation. A preview is not a permanent authorization.
- Execute the locked evaluation protocol in PRODUCT.md independently for each provider.

Exit: user supplies a new project, AI A changes a file and proposes a decision, owner approves, AI B identifies the correct latest decision, prior action result and next step without manual copying. Reverse provider order on another project. Restart and repeat. Changed, conflicting, unavailable and deleted evidence produce correct behavior. Evaluation thresholds pass; fixtures or scripted replies cannot satisfy the real-provider proof.

### G4 — User control, Passport and one real external client

Maps to AID-009. Depends on G3; external client is additional to the two in-app adapters.

- Implement selective preview/export/import with schema version, provenance, integrity hashes, source versions and explicit inclusion rules. A manifest-only export is not a self-contained round trip: include authorized source content or clearly identify unresolved external references.
- Imported decisions retain their origin but require approval in the destination; import never grants access or executes instructions. Deduplicate repeat imports; reject unsupported schemas, invalid references and oversized payloads without partial silent acceptance.
- Provide one tested MCP connection with narrow context-read and explicit handoff tools calling the same service authorization. Record client/version/transport and actual capture coverage. Use scoped revocable credentials for local stdio; implement the applicable MCP authorization flow before offering remote HTTP. Do not assume installed SDK/client compatibility with the newest specification.
- Implement deletion across files/messages, source links, extracted text, indexes, caches, summaries and exports created subsequently. Immediately deny retrieval after deletion commits; track physical cleanup separately. Reapply deletion records before a restored backup can serve requests. Previously downloaded/provider-held copies cannot be recalled.

Exit: export/import into a different authorized workspace preserves chosen evidence and versions without importing authority; excluded material stays excluded. Repeated import does not duplicate content. The external client retrieves approved context and records a handoff; revoked grants block it. Deleted content cannot be recovered through product retrieval paths.

### G5 — Release candidate reliability and usability

Maps to M5 engineering readiness. Depends on G4.

- Run end-to-end acceptance against the actual web/API/database/storage stack with two providers. Keep unit tests for complex rules, real restricted-role integration tests for policies and transactions, and a small UI suite for critical journeys. Mocks test failure handling; they do not establish integration success.
- Freeze the reference environment and run PRODUCT.md load targets. Collect request IDs, latency, queue depth, failures, usage and budget events without full private prompts. Define expected alerts and recovery actions.
- Exercise database outage, unavailable storage, disk-full behavior, interrupted write, stuck extraction, provider failure and duplicate delivery. Ensure bounded retries and inspectable failed jobs. Perform and time an isolated backup restore.
- Review the actual six screens at desktop/mobile widths, keyboard-only navigation, visible focus, accessible names, error announcements and a screen-reader smoke pass. Use WCAG 2.2 AA as the target; automated checks alone are insufficient for a conformance claim. No dead primary control or success message before confirmed persistence.
- Conduct a separate review of changes to permissions, privileged database functions, secret handling, import/export and untrusted content. Re-run relevant checks after fixes; do not accept a stale green run.

Exit: all mandatory technical gates pass on an identified commit; evidence includes known limitations, unresolved low-severity issues and supported environment. A failed critical path blocks pilot release. Release rollback must preserve durable data; destructive down-migrations are not a default recovery plan.

### G6 — Observed pilot value and scored release decision

Maps to AID-010. Recruitment planning can start earlier; actual invitations/deployment/spend follow their authorization boundaries.

- Recruit five people with ongoing multi-AI projects; observe their existing workflow before the product demonstration. Record consent and minimize collected private content.
- Run the matched task and first-use protocols in PRODUCT.md, count failure/approval time, and observe seven-day independent return use. Ask for willingness to pay for a specific offer rather than generic enthusiasm.
- Score all 20 rubric items from current evidence and apply mandatory gates. Publish internally the actual numerator/denominator, language/provider breakdown, cost and limitations. Do not rewrite thresholds after seeing results to produce 9/10.
- If users do not save time, isolate retrieval quality, approval burden or workflow mismatch; revise and retest. If engineering passes but demand does not, call it a technically ready pilot with unvalidated value, not a completed business.

Exit: >=90/100, all mandatory gates, a reproducible real demonstration and observed pilot results. A broad launch remains a separate decision.

## First three implementation work packets

1. Foundation repair: F01/F02/F03 with failing-before/passing-after regressions; retain existing data and separate test credentials. No UI expansion needed.
2. First durable journey: F04/F05 plus identity, isolated workspace and upload/read/version persistence from G1. Demonstrate after restart through the actual web app.
3. First shared-understanding journey: G2/G3 in the smallest complete flow, then finish G4-G6. Avoid adding more provider logos, media formats or agent orchestration before this works.

Each packet includes implementation, meaningful checks, a separate review pass and an evidence update. Start only one schema-changing packet at a time in this checkout; integrate reviewed changes before the next migration.

## Planning envelope and dependency risks

Estimate, not commitment: with one experienced implementer using AI assistance, working credentials and no major environment blockers, budget G0 2-4 focused engineering days, G1 5-8, G2 4-7, G3 7-12, G4 4-7 and G5 4-7: 26-45 engineering days before pilot evidence. G6 needs at least a seven-day return-use observation after onboarding, plus recruitment time. Re-estimate after G1 using actual throughput; no fixed launch date or 30-day completeness promise.

Highest risks: provider access/budget can block real verification; capture integrations may expose less than expected; retrieval may underperform in Armenian; manual approval may erase time savings; shared data deletion/backup restoration can reintroduce stale content. Each has an explicit gate above. Keep the full unified workspace vision; narrow implementation order, not the promised ownership and continuity model.

## Evidence register

| Evidence | Current status | Scope / next action |
| --- | --- | --- |
| Prototype state checks and web lint | Passed in latest review | Scripted workflow only; not a backend release gate |
| Existing PostgreSQL isolation suite | Prior contributor reports 20 passes; subset independently checked | Reviewer confirmed hidden foreign files, denied editor escalation and revoked-member read denial; full recreation not rerun |
| File attribution regression | Failing-before/passing-after in isolated PostgreSQL 17.12, 2026-11-18 | Migration 2 protects identity/creator/creation time, server stamps timestamps; per-edit version/event model still G1 |
| Test password / repeated setup defects | Fixed and exercised, 2026-11-18 | Private cluster; unchanged application login on repeat bootstrap; SQLx ledger/checksum/rollback/concurrency/adoption checks pass |
| Prototype browser checks | Reported by prior contributor | Not independently reproduced by reviewer; no actual app journey established |
| G0-G6 acceptance evidence | Not collected | Record commit, date, environment, command/task, result and artifact per gate |

Required future evidence fields: gate/check ID, source commit, supported environment, reproducible invocation, expected/actual result, provider/model/config when relevant, redacted artifact location, reviewer and unresolved limitations. Do not commit private pilot data, provider secrets or raw private prompts as evidence.

### First packet evidence — 2026-11-18

Implementation: `apps/db-tool` (SQLx 0.8.6), additive migration `0002_file_provenance.sql`, private-cluster shell runner, non-rotating bootstrap, regression SQL, updated CI recipe and setup instructions. Migration 1 was not edited. Separate review pass added explicit connection-target validation, server creation timestamps, PostgreSQL binary selection and byte-preserving migration attributes.

Observed locally: `cargo fmt --all -- --check`, `cargo clippy --locked -p aidrive-db --all-targets -- -D warnings`, and the real PostgreSQL suite invoked with `AIDRIVE_MIGRATOR=<absolute WSL binary> bash scripts/db-check.sh` passed. The crate's unit-test target currently contains zero tests; behavioral validation comes from the PostgreSQL integration suite. The suite exercises 20 original isolation assertions on each of old and fresh-upgraded schemas, five provenance assertions after each upgrade path, plus migration/bootstrap/rollback/concurrency negatives. Full output: [foundation check log](../work/aidrive-g0-foundation-checks.log); [source fingerprints](../work/aidrive-g0-source-fingerprints.json). No commit exists yet; the fingerprints identify this checked working-tree snapshot.

Environment: WSL Ubuntu 24.04, PostgreSQL 17.12, Rust 1.98.0. Fresh/legacy fixtures exist only in private temporary instances and are removed after testing. The existing `aidrive`/`aidrive_check` databases were not upgraded, deleted or password-rotated. Remote CI, Compose, native Windows migration execution and the declared Rust 1.88 compiler itself were not tested. Next work packet: F04 live web/API/database readiness and remaining F05 evidence, then G1 identity and durable Drive.

### F04 evidence — 2026-11-18

Implementation: `aidrive-api` connects through a lazy SQLx pool from `DATABASE_URL` (runtime role only). `/healthz` stays process liveness. `/readyz` returns 200 only when the database answers, the connected role is not superuser/`BYPASSRLS`/table owner, and required schema objects from migrations 1-2 exist (catalog probe; `aidrive_app` cannot read the SQLx ledger). Failures return 503 with check names only; logs record an error kind, not driver messages. The web shell polls `/api/readyz` through the Next proxy and distinguishes ready, API-up-database-not-ready and API-offline, exposed as `role="status"`. File storage is not yet a readiness dependency because it does not exist (G1).

Observed (WSL Ubuntu 24.04, PostgreSQL 17.12, Rust 1.98.0; web on Windows Node via Next dev proxy):
- `cargo test -p aidrive-api`: 4 unit tests pass (no database configured; unreachable database without URL/password in body; liveness independent of database; malformed URL not echoed).
- `bash scripts/api-check.sh` (private disposable instance): migrated + restricted role 200 ready; superuser 503 `role: failed`; schema with only migration 1 503 `schema: failed`; wrong password 503 `database: failed`.
- Live journey: browser -> `localhost:3000` -> proxy -> API in WSL -> private PostgreSQL. Indicator read "API and database ready" (200); after stopping PostgreSQL, "API up, database not ready" (503) with `/healthz` still 200; after stopping the API, "API offline" (proxy 500). Private instance removed afterwards; development databases untouched.
- `cargo fmt --check`, `cargo clippy -p aidrive-api -p aidrive-db --all-targets -D warnings`, `npm run lint`, `npm run build` pass.

Not verified: Compose, remote CI (`api-check.sh` added to the database job), native Windows API against PostgreSQL, clippy for `aidrive-mcp-server` in this pass. F05 remains open.
- MCP (2026-11-18): `aidrive-mcp-server` adds read-only `get_api_readiness`. Native Windows binary passed an stdio handshake (initialize, tools/list, both tool calls) against the WSL API on a new `aidrive_dev` database, returning `ready`. Not yet observed inside Claude Code or Codex clients.

### F05 status — 2026-11-18

- Clean baseline: first commit `dd6d0b5`; full-history scan for private keys, provider/API tokens, cloud keys and personal identifiers found none. Only documented local-development credentials are committed.
- CI: `database` job runs `db-check.sh` and `api-check.sh`; written, not yet observed remotely.
- Supported local startup path exercised: WSL PostgreSQL 17 + `scripts/dev-api.sh` + Next dev proxy. Isolated test environment exercised: `with-test-postgres.sh` private instances.
- Blocked, not failed: Compose needs Docker on this machine; remote CI needs an authorized push. G0 stays open until both are observed. G1 local work proceeds because neither blocker is a G1 dependency.

### G1 identity slice — 2026-11-18

Decision: [ADR 0002](docs/adr/0002-authentication.md) — Argon2id passwords (OWASP minimum) and opaque server-side sessions now; passkeys required before any hosted pilot.

Implementation: migration `0003_auth.sql` (credentials and sessions with no `aidrive_app` table privileges; five narrow definer functions; lifetimes absolute 7 days / idle 24 hours in the database); `/auth/register|login|logout|session`; `__Host-aidrive_session` cookie storing only a SHA-256 of a 256-bit token server-side; Fetch Metadata/Origin CSRF guard on every non-GET; equal-cost login for unknown emails; sign-up off unless `AIDRIVE_ALLOW_SIGNUP=1`; web sign-in/sign-up panel with loading, error and unavailable states. `/readyz` now also requires migration 3 objects.

Observed:
- `db-check.sh`: all prior foundation checks plus A01-A08 (no app-role table access, normalized/duplicate email, argon2id-only hashes, unknown-user session denial, resolve/revoke) and idle/absolute expiry after server-side aging; legacy adoption upgrades through 0003. Temporary ledger-test migrations renumbered to 0900/0901 to avoid colliding with 0003.
- `cargo test -p aidrive-api`: 12 pass (password rules, token/cookie format, Argon2id parameters, CSRF accept/reject matrix, signup gate, unauthenticated session).
- Live in the browser via the Next proxy against `aidrive_dev`: short password rejected client- and server-side; account created (201); `__Host-` Secure HttpOnly cookie accepted on `http://localhost`, invisible to `document.cookie`; session survived page reload and an API process restart; logout made the session 401; wrong password and unknown email both 401 `invalid_credentials`; mixed-case email login succeeded. Database held only the Argon2id PHC string and 32-byte token hashes. Direct cross-site POSTs returned 403.

Not done in G1 yet: rate limiting, breached-password corpus, passkeys/recovery, session use for workspace queries (workspace creation/listing, agent principals and grants), last-owner protection, files/versions/upload. The local test account lives only in gitignored `.env.dev-account`.

### G1 workspace slice — 2026-11-18

Implementation: migration `0004_workspace_access.sql` (`app.my_workspaces()`; last-owner trigger with workspace-row lock, cascade-safe). API `GET/POST /workspaces`, `GET /workspaces/{id}` behind session auth; RLS context set only from the resolved session inside a transaction (`set_config(..., true)`); unknown and foreign IDs both 404. Web: "Your spaces" panel with loading, empty, error and create states; header "New space" is enabled only when signed in and focuses the form. `/readyz` now requires migration 4 objects.

Observed:
- `db-check.sh`: W01-W07 (no-context listing empty, exact per-user listing with role, context gone after commit, sole owner cannot leave or be demoted, hand-over keeps an owner, workspace deletion cascades) plus a two-session race in which owners demote each other concurrently: exactly one owner remains and the second demotion is refused. The race lock was not mutation-tested.
- `api-check.sh`: readiness cases plus a live two-user flow through the real router on a one-connection pool: create/list/show, other user's list empty and GET 404, unknown ID 404, blank name 422, forged and missing cookies 401, and no `app.*` setting left on the reused connection. Mutation check: switching `set_config` to session scope makes that last assertion fail.
- `cargo test -p aidrive-api`: 14 pass; clippy/fmt/lint/build pass.
- Browser via Next proxy against `aidrive_dev`: signed in, empty state shown, "New space" focused the form, space created and listed as owner, blank name 422, list intact after an API restart and reload.

Next in G1: agent principals with scoped grants and revocation (a grant cannot exceed the granter's rights), then files, immutable versions, conditional writes and restore.

### Integrated review fixes — 2026-11-18

Applied to the main checkout after the founder confirmed Claude Code had stopped editing. Base: `aea0f66`. Three independently reproduced defects are fixed:

- Login rejects passwords above 256 Unicode characters with the same invalid-credentials response; it no longer authenticates a 256-character prefix while discarding an incorrect suffix. Real-router/PostgreSQL tests cover both ASCII and Armenian at the accepted boundary, rejection without issuing a cookie, and successful login with the complete correct password.
- Readiness requires the intended `aidrive_app` role and rejects CREATEDB/CREATEROLE, membership in `aidrive_owner`, and the previously rejected privileged/owning roles. Live tests cover a different unprivileged role returning 503, owner-membership escalation returning 503, and recovery after removing membership. This is not a complete per-object permission or schema-drift audit.
- Logout switches the UI to signed out only after HTTP 204. Failed/ambiguous requests leave the existing view with an accessible error and retry. The shared request helper has eight Node tests, and the live API test verifies cookie revocation and safe repeat logout. Browser rendering of these specific changes has not been independently exercised.

Validation: `cargo test --locked -p aidrive-api` (14 pass; three live tests skipped in that command but exercised by `scripts/api-check.sh`); seven live readiness configurations, two-user workspace flow, and ASCII/Armenian login/logout regression all passed in a private PostgreSQL instance. Formatting, API Clippy, web lint, eight Node tests and production webpack/TypeScript build passed. Initial sandboxed webpack invocation failed without a useful compiler diagnostic; the permitted unsandboxed rerun passed. CI now runs `npm test`; remote CI remains unobserved. No migrations, development database rows or live account credentials were changed by these fixes.

Remaining G1 scope is unchanged: agent principals/grants/revocation and durable file/version workflows. Authentication still needs rate limiting, broader breached-password protection and the ADR's hosted-release requirements.

### G1 agent metadata grant slice — 2026-11-18

Implemented: migration `0005_agent_grants.sql`; session-authenticated owner management at `/workspaces/{id}/agents`; separate bearer-only `GET /agent/workspaces/{id}`; web creation, one-time secret display, metadata listing and revocation. The only supported scope is `workspace:read`, returning the selected workspace's ID/name and agent ID. Tokens contain 256 random bits, are stored as SHA-256 hashes, expire after seven days and never appear in listing responses. The API rejects unsupported scopes/fields and returns `Cache-Control: no-store` on agent routes. Request UUID uniqueness prevents duplicate creation on retries; lost secrets must be revoked and replaced. Readiness now also requires migration 5's read function.

Owner demotion/removal permanently revokes their keys; promotion or rejoining does not revive them. Reads lock membership then grant, recheck expiry after waiting, and never impersonate the owner. A committed revocation blocks a subsequent read; responses already admitted may still arrive and cannot be recalled. Research, ordering and tradeoffs are recorded in `docs/ARCHITECTURE.md`; database boundaries in `docs/DATABASE.md`; routes and response shapes in `contracts/openapi.yaml`.

Observed on the final implementation:
- `scripts/api-check.sh`, private PostgreSQL 17.12: seven readiness configurations, existing two-user workspace/password/logout flows, plus agent grant tests through the real router on a single-connection runtime pool. Covered owner/viewer/editor/outsider management, wrong-workspace reads, bearer/cookie separation, extra-scope rejection, hash-only storage and denied direct-table access, duplicate issuance, repeated revocation, expiration, demotion/re-promotion, removal/rejoining and pooled-context cleanup.
- Concurrent read was observed waiting behind an uncommitted revocation via `pg_stat_activity`, then denied after commit. A second read waited behind a grant row lock while its expiry elapsed; it was denied even though the holder did not modify the row. These are real overlapping database transactions, not mocked timing results.
- `scripts/db-check.sh`: fresh and legacy upgrades through migration 5, existing isolation/provenance/auth/workspace tests, last-owner race, checksums, rollback and migration concurrency all passed.
- Rust: 14 unit tests passed; four ignored live tests were exercised by the integration script. API Clippy with warnings denied and formatting passed. Web lint, eight existing logout helper tests and production Next.js/TypeScript build passed. The existing web tests do not cover the new agent panel's rendering.

Limits: this slice was verified in disposable database instances; the development database has not been upgraded. Browser interaction of the new panel, Compose and remote CI remain unobserved. No provider connection, MCP permission integration, file/memory scope, activity audit stream or hosted rate limit is claimed. G1 remains open. Next implementation: durable file bytes, immutable versions, conditional writes, restore and corresponding agent permissions; then verify the complete authenticated Drive journey after restart.

### Independent review of agent grants and text file versions — 2026-11-19

Reviewer: Claude Code (implementation by the other contributor; not modified in review). Static reading of `0005_agent_grants.sql`, `agents.rs`, `0006_text_file_versions.sql` and `files.rs` found no blocking defect. Lock order (membership, then grant/file) matches revocation and demotion; definer functions pin `search_path` and revoke `PUBLIC`; tokens are hash-only; writes require editor/owner; bytes, head and action event commit in one transaction (text stored in PostgreSQL, 256 KiB cap), which removes the staged-bytes/metadata gap for this file class.

Re-run on 2026-11-19, WSL Ubuntu 24.04, PostgreSQL 17.12: `cargo fmt --check`, API/db-tool Clippy `-D warnings`, 16 unit tests, `db-check.sh` (all foundation checks), `api-check.sh` (seven readiness configurations; file create/read/conditional-write race/restore/tamper/rollback/restart; agent isolation/expiry/revocation/concurrency; workspace and password/logout flows), web lint, 8 Node tests and production build — all passed.

Non-blocking notes: (1) `agent_grants.issued_by ON DELETE CASCADE` erases grant history when a user is deleted; keep an audit trail before deletion flows exist. (2) `issuer_unavailable` is effectively unreachable because demotion already revokes. (3) "A grant cannot exceed the granter's rights" holds only because the single scope is read; enforce it explicitly when write scopes arrive. (4) Tests are pulled in with `include!`; a `mod` is cleaner. (5) No rate limit on the bearer endpoint yet. Browser interaction with the agent and files panels was not exercised in this review.

## One-week MVP (founder decision 2026-11-19): MCP-first continuity

Goal by 2026-11-26: Claude Code and Codex, the founder's actual tools, share one AIDrive workspace through MCP and continue each other's work from files and handoffs without manual copying. In-app provider adapters (G2) and the full memory workflow (G3) follow the MVP; the G0-G6 gates are unchanged and are not claimed by the MVP.

| Day | Deliverable | Status |
| --- | --- | --- |
| 1 | Agent file scopes, bearer file API, MCP file tools | Done 2026-11-19 (below) |
| 2-3 | Handoff records (summary, decisions, next step, exact source versions) with owner approval; MCP `get_latest_handoff`/`record_handoff` | Done 2026-11-19 (below) |
| 4 | Activity timeline in the web app (person/agent, action, version) | Done 2026-11-19 (below) |
| 5 | Real demo: Claude Code changes a file and records a handoff; Codex continues; restart; repeat in reverse | Partly: real Codex desktop continued from AIDrive (day 5b); full two-client loop pending |
| 6-7 | Fixes from 1-2 real users; setup guide | Session memory built 2026-11-20 (founder-prioritised); user sessions pending |

### MVP day 1 evidence — 2026-11-19

Implementation: migration `0007_agent_file_access.sql` — scope levels `workspace:read` < `files:read` < `files:write`; `app.begin_agent_request` validates the grant (issuer still owner, not revoked/expired, sufficient scope; membership-then-grant lock order) and sets transaction-local issuer/workspace/agent context; `agent_id` on `file_versions`/`file_actions` is set by trigger and honoured only for the issuer's own active `files:write` grant; deleting a grant that authored versions is refused. API: one `Principal` extractor serves the existing file handlers for sessions (`/workspaces/...`) and bearer agents (`/agent/workspaces/...`); `/agent/*` is exempt from the browser Origin check because it accepts only an explicit `Authorization` header and ignores cookies; grant issuance accepts `scope`. MCP server 0.0.2: `get_workspace`, `list_files`, `read_file`, `file_history`, `create_file`, `update_file`, idempotent request IDs, recovery guidance on errors, and server instructions telling models to treat file content as data.

Observed:
- `db-check.sh`: F01-F10 (owner-only issuance, unknown scope rejected, unknown/foreign tokens, workspace:read cannot read files, files:read reads through RLS but cannot open a write context, agent attribution on version and action, human writes unattributed, forged `app.agent_id` refused, revoked writer blocked) plus all prior checks.
- `api-check.sh`: new live router flow (bearer-only create/read/edit; read key 403 `insufficient_scope`; stale base 409; key bound to its workspace; cookies ignored on agent routes; owner history shows `agent_id`; revocation blocks the next request; no pooled context leak) plus all prior live flows.
- Unit: API 16, MCP 2; fmt and Clippy `-D warnings` for API, db-tool and MCP.
- End to end on `aidrive_dev` (migrated through 0007) with the native Windows MCP binary as two processes holding "claude" and "codex" `files:write` keys: Claude created `handoff.md` (rev 1, claude agent); Codex listed, read and appended (rev 2, codex agent); Claude's stale overwrite returned 409 with recovery guidance; history listed both agents. Keys live only in gitignored `.env.dev-agents`.

Not yet: observed inside the real Claude Code and Codex clients; handoff records; web display of agent attribution; agent rate limits; keys issued by editors.

### Durable Drive UI acceptance and draft protection — 2026-11-19

Completed the human text-file slice and verified the current checkout including `71243ef`. The web UI uploads UTF-8 `.txt`/`.md` files up to 256 KiB, edits with an explicit base version, displays history and side-by-side comparisons, downloads saved text and restores an old version by appending a new one. Ambiguous saves retain the original request/body for retry. A guard warns before discarding a draft or unresolved save on file/space changes, sign-out or page unload. Creating another file no longer automatically replaces an already-open editor. Drafts remain in memory; this is not offline persistence.

Observed locally:
- Real Chrome -> Next proxy -> Rust API -> private PostgreSQL: created an account/workspace; uploaded Armenian UTF-8 text; saved version 2; restored version 1 as version 3; reloaded and reopened with the restored content/history intact. No mock backend was used.
- With a draft open, a second real API write moved the head. UI save returned 409 and retained the draft. Comparing with the latest, explicitly adopting that base and saving succeeded as the next version. Cancelling the sign-out warning retained the authenticated editor and draft.
- At 390px viewport width, visual inspection caught the inherited icon-only 34px primary-button rule clipping the save label. Form controls now retain text-sized widths; the recheck measured the save button at 133 x 40px with no horizontal page overflow. Screenshot: [mobile file editor](../output/playwright/aidrive-file-editor-mobile-fixed.png).
- `scripts/api-check.sh` rerun on `71243ef` passed all readiness, text-file, restart, agent-file, agent-grant, workspace and authentication checks. Output: [API check log](../work/aidrive-g1-files-final-api-check.log). Earlier migration preservation checks passed after comparing all original columns and separately checking the new nullable head; [database check log](../work/aidrive-g1-files-db-check.log).
- Web lint and eight existing Node logout tests passed; production webpack/TypeScript build passed. Those Node tests do not cover the file editor; the observations above are browser acceptance evidence. The local test browser and private API/database fixtures were stopped after testing.

Documented all implemented human/agent file paths and response schemas in `contracts/openapi.yaml`, including byte limits, pagination, idempotent replay, conflicts and attribution. Corrected grant-scope documentation to include migration 0007. Architecture/database docs now explain PostgreSQL-backed text and its tradeoff against a separate object store. Agent restore/download routes are not advertised because they are not implemented.

G1 stays open: larger/binary storage, quotas/rate limits, broader release identity requirements and the full agent/real-client acceptance gates remain. The next MVP packet is the already-planned approved handoff workflow; this UI verification does not claim shared memory or production readiness.

### MVP day 2-3 evidence — 2026-11-19

Implementation: migration `0008_handoffs.sql` — immutable handoffs (summary, up to 20 decisions, next step) with exact `(file_id, version_id)` sources; `app.record_handoff` (editor/owner or files:write agent, agent attribution via 0007's checked context, idempotent request IDs); `app.decide_handoff` (current owner only, refused in any agent context, locks cited files and requires them unchanged, supersedes the prior approval, decisions are final). Staleness is computed on every read from current file heads, so changed evidence is flagged before the next delivery without a background job. API: list, latest (approved + unapproved proposals), record (session and `/agent/`), decide (session only). MCP 0.0.3: `get_latest_handoff` states status in plain words before the data (approved-current, approved-but-stale with changed files named, unapproved proposals), `record_handoff`. Web: handoffs panel with status, decisions, next step, cited files (changed ones highlighted) and owner Approve/Reject (approve disabled when stale); the AI-access panel now lets the owner choose the key's permission.

Observed:
- `db-check.sh`: H01-H11 (viewer cannot record; agent attribution and replay idempotency; reused request ID rejected; agent cannot approve even with the owner's write key; cross-workspace source rejected; editor cannot approve; owner decides once; content not rewritable; approval refused after the cited source changed; approved handoff detectably stale; newer approval supersedes) plus all prior checks.
- `api-check.sh`: live router flow (agent proposes with sources; read key 403; no agent decision route; owner approval final; stale flag after a file change; stale approval 409 `stale_sources`; unknown source 422) plus all prior flows. Unit: API 16, MCP 3; fmt/Clippy clean; web lint and build pass.
- Live on `aidrive_dev` (migrated through 0008), native Windows MCP binary, real browser: Claude (MCP) read `handoff.md` rev 2 and recorded a handoff citing it; Codex's `get_latest_handoff` reported no approved handoff and one unconfirmed proposal; the owner approved it in the web panel (200, "Approved · current"); Codex then received the approved decisions and next step, continued by editing `handoff.md` to rev 3, and `get_latest_handoff` immediately reported the approval as stale naming `handoff.md`; the web panel showed "Approved · sources changed since". Console showed only the two expected 401 responses from the signed-out session check before sign-in.

Not yet: activity timeline (day 4), observation inside the real Claude Code/Codex clients, keyboard/screen-reader pass of the new panel, editing a decision after approval (a new handoff is the path).

### MVP day 4 evidence — 2026-11-19

Implementation: migration `0009_activity.sql` adds `app.workspace_activity(workspace, before, limit)`, a read-only timeline derived from `file_actions`, `handoffs` and `agent_grants` (no second log that could drift). It requires viewer membership of the selected workspace or an agent context for it, and returns member emails and agent names that ordinary RLS hides from other members. API `GET /workspaces/{id}/activity` and `/agent/workspaces/{id}/activity` (files:read or higher), cursor paging with a validated `before`. MCP 0.0.4 `get_activity` returns one plain-language line per event. Web: activity panel per space, AI events marked and attributed as "assistant · key from owner", older-page loading.

Observed:
- `db-check.sh`: V01-V05 (full timeline newest first; agent events name the agent and accountable owner, human events carry no agent; limit/cursor paging; non-member and no-context get not-found) plus all prior checks.
- `api-check.sh`: live router flow (owner and files:read agent see attributed events; workspace:read key 403; outsider 404; malformed cursor 422; paging excludes earlier pages) plus all prior flows. Unit: API 17, MCP 4; fmt/Clippy clean; web lint and build pass.
- Live on `aidrive_dev` (migrated through 0009): Codex's `get_activity` and the web panel both listed the real day 1-3 history line for line — keys issued, Claude created `handoff.md`, Codex rev 2, Claude proposed, owner approved, Codex rev 3. No horizontal overflow at 375 px; the list collapses to one column. The browser pane was hidden during this check, so evidence is DOM text and metrics, not screenshots.

Not yet: real Claude Code/Codex clients; keyboard/screen-reader pass of the new panels; workspace-creation and membership events in the timeline.

### MVP day 5a: desktop pairing — 2026-11-20

Founder direction: the desktop Claude Code and Codex apps should connect when told to, without anyone copying keys. The CLI-only demo was stopped: the local `claude` CLI login had expired and `codex exec` waited on stdin.

Research (2026-11-20): Claude Code docs ([MCP](https://code.claude.com/docs/en/mcp)) — `claude mcp add [options] <name> -- <command> [args]`, `--scope user` stored in `~/.claude.json`; the [desktop page](https://code.claude.com/docs/en/desktop) does not state which MCP file the Code tab reads, so that is unverified. Codex MCP docs (developers.openai.com/codex/mcp, now redirected) — `codex mcp add <name> -- <command>`, configuration shared by the desktop app, CLI and IDE extension. [RFC 8628](https://www.rfc-editor.org/rfc/rfc8628) §5.1/§5.4/§6.1 — rate-limited high-entropy user codes, short lifetime, show the client and ask the user to confirm the code.

Decision: device-style pairing in which the client generates the secret and sends only SHA-256; approval creates an ordinary agent grant for that hash. Tradeoff vs. a remote MCP connector with OAuth: works today with both desktop apps over local stdio and keeps the existing grant model, but the assistant must be able to run one command, and it is local-only until hosting exists.

Implementation: migration `0010_agent_pairing.sql` (pairings and failed-lookup log, no app-role table access; 8-letter codes from 20 consonants by rejection sampling; 10-minute expiry; global cap of 200 live requests; ten failed code lookups per user per ten minutes, counted even through the decision path because it returns rather than raises). API `/agent/pairing` start and secret-bound status; session `/pairing/{code}` lookup and decision. Web `/pair` page (signed-out, unknown/expired, already-decided, no-owned-space and done states; shows the client and code and asks the owner to confirm the match). MCP 0.0.5 `connect --client claude|codex`: generates the secret, opens the approval page, waits (resumable), writes a per-user profile, registers via each client's official `mcp add`; `serve --profile NAME` reads it, so keys never appear in client configs.

Observed:
- `db-check.sh`: P01-P09 (code format and replay, same ID with another secret refused, anonymous lookup refused, status needs the secret, normalized lowercase/dashed code, non-owner cannot approve, owner approval issues a working agent key, decided once, denial issues nothing, guessing blocked after ten failures including through the decision path, targeted request untouched) plus all prior checks.
- `api-check.sh`: live router pairing flow (hash-only start, replay, secret-bound status, another user cannot approve into a space they do not own, paired secret works as an agent key, decided once, unknown code 404) plus all prior flows. Unit: API 18, MCP 7; fmt/Clippy clean; web lint/build pass.
- Live: the installed release binary's `connect --client codex --no-register --no-browser` (isolated profile folder) printed a code and link; the browser pane showed the signed-out prompt, then after sign-in the client name, matching code, expiry and space/permission choices; Allow completed the command on its own ("Approved by owner@aidrive.test for space AIDrive launch (files:write)"); `serve --profile codex` with no key in the environment returned the workspace and the latest handoff state.

Not yet: real registration in the founder's Claude Code and Codex desktop apps and a run inside them; whether the desktop Code tab reads `~/.claude.json`; profile protection on Windows relies on the user-profile ACL (Unix gets 0600); no per-IP limit on starting pairings.

### MVP day 5b: real desktop clients — 2026-11-20

- Claude Code desktop: `connect --client claude` was run from this Claude Code session; the owner approved code WJZR-VPVG in the browser; `claude mcp list` reports `aidrive ... serve --profile claude - Connected`. Tools load only in new sessions; a new-session run inside Claude Code has not been reported yet.
- Codex desktop: its sandbox (`workspace-write`, Windows `sandbox = "elevated"`) could not see `%LOCALAPPDATA%\AIDrive\bin`, so pairing was started outside the sandbox; the owner approved FRLF-ZPMZ; `codex mcp list` shows `aidrive` enabled. **In a new Codex chat the founder observed `get_workspace`, `get_latest_handoff` and `get_activity` succeed; Codex stated the workspace (AIDrive launch, files:write), the target audience and the 10-minute interview script from `handoff.md` without any manual context.** This is the first observation of a real external client continuing from AIDrive state.

### Session memory — 2026-11-20

Design and research: [docs/research/session-memory.md](docs/research/session-memory.md) (six projects compared; 13 techniques adopted as ideas, no code copied).

Implementation: migration `0011_sessions.sql` (sessions + events, no app-role writes, generated `tsvector` with the `simple` configuration, idempotent event keys, owner-only delete, files:write agents capture, files:read agents read). API list/upsert/events/search/excerpt for sessions and `/agent/`, owner delete. Search: any term may match, 3+ character terms match as prefixes (Armenian inflection: "ֆայլ" matched 2 events exactly vs 32 as a prefix on real data), phrases and exclusions honoured, `ts_headline` snippets. MCP 0.0.6: `capture --client claude|codex --project <folder> [--dry-run] [--hook]` with hand-rolled local redaction (no new dependency); tools `get_project_brief`, `search_sessions` (whole hits packed into a token budget), `get_session_excerpt`. Web: captured-sessions panel with search, highlighted snippets and owner delete.

Defects found by running on real transcripts, each fixed with a regression test: a crash on punctuation-only words; `PGPASSWORD=...` and multi-pair words (`{email:'x',password:'y'}`) escaping redaction; capture of 23 Codex internal guardian-review threads (now skipped by `session_meta.thread_source`); all-terms search returning nothing for natural queries; hook mode not scoped to `--project` (a user-level hook would have uploaded every project).

Observed:
- `db-check.sh`: S01-S09 (merge on re-capture, idempotent events, coverage and agent attribution, English/Armenian/prefix search, viewer cannot capture or delete, files:read cannot capture, files:read agent reads but cannot delete, outsider sees nothing, events not rewritable, owner deletion removes events and search entries) plus all prior checks.
- `api-check.sh`: live session flow (idempotent capture and replay, ranked search with snippets, any-term and prefix matching, phrase strictness, excerpt, coverage, scope and role checks, no agent delete route, outsider 404, deletion removes hits) plus all prior flows. Unit: API 19, MCP 15; fmt/Clippy clean; web lint/build pass.
- Real data on `aidrive_dev`: dry run then import of the project's Codex sessions — 1 user thread, 379 events, 2 values redacted; replay inserted 0; leak scan over stored text found 0 agent keys, 0 `sk-` keys, 0 private keys, 0 `password=value` pairs, 0 dev passwords, 0 reviewer-thread lines. `search_sessions("SQLx migration ledger")` through Codex's real profile returned the turns where the migration-ledger decision was made, and `get_session_excerpt` returned their context. Web panel listed the session; an Armenian search with no occurrences correctly returned nothing.

Not yet: an installed Claude Code `Stop` hook run; Claude Code sessions inside the project folder (none exist yet — this session runs from the parent folder); retention policy; capture consent per project in the UI; evaluation against the held-out memory protocol.

## Session checkpoint — 2026-11-20 (host shutdown)

Everything below is committed; nothing is pushed. Resume from here.

State:
- Code: last feature commit `bd541fe` (session memory). Migrations 0001-0011. All suites passed at that commit (`scripts/db-check.sh`, `scripts/api-check.sh`, API 19 / MCP 15 unit tests, web lint/build).
- Development database: `aidrive_dev` in the WSL Ubuntu PostgreSQL 17 cluster (port 5432), migrated through 0011. Contains the owner account, the "AIDrive launch" space, files, handoffs, agent keys and the imported Codex session (379 events).
- Local secrets (gitignored, never commit): `.env.dev-account` (owner login), `.env.dev-agents` (old claude/codex keys from day 1). Paired client profiles: `%APPDATA%\AIDrive\profiles\claude.json`, `codex.json`.
- Installed MCP binary: `%LOCALAPPDATA%\AIDrive\bin\aidrive-mcp-server.exe` (0.0.6). Registered as `aidrive` in Claude Code (user scope) and Codex (`codex mcp list`). Codex desktop was observed using the tools in a new chat.
- Codex config was changed by the founder to `sandbox_mode = "danger-full-access"`, `approval_policy = "never"`.

Restart after reboot:
1. `wsl -d Ubuntu -u root -- service postgresql start` (if the cluster is not running).
2. `wsl -d Ubuntu -- bash -lc "cd <checkout in WSL> && CARGO_TARGET_DIR=target/wsl CARGO_NET_OFFLINE=true AIDRIVE_ALLOW_SIGNUP=1 bash scripts/dev-api.sh"` (API on 127.0.0.1:8080; wait for `/readyz` = ready).
3. Web: `npm run dev` in `apps/web` (http://localhost:3000).
4. Builds that need crates offline use WSL with `CARGO_TARGET_DIR=target/wsl CARGO_NET_OFFLINE=true`; the MCP binary builds natively on Windows (`cargo build --release --offline -p aidrive-mcp-server`). To replace the installed binary while a client runs it, rename the old exe first.

Open work, in order:
1. Top-20 memory analysis is done: [docs/research/ai-memory-findings.md](docs/research/ai-memory-findings.md) (19 projects, synthesis, 15 techniques, revised plan M1-M6).
2. Founder decision on the revised memory plan order (M1 knowledge items + review inbox → M2 freshness/validity/conflicts → M3 brief v2 → M4 visibility → M5 evaluation → M6 automatic capture).
3. Install the Claude Code Stop hook for automatic capture (founder asked to defer; not installed).
4. Day 5 full loop: Claude Code records a handoff in a new session, Codex continues, restart, reverse order.
5. G0 still blocked on Compose (no Docker) and remote CI (needs authorized push).

### MVP day 5c: Claude Code desktop continues from AIDrive — 2026-11-20

After a host restart, a new Claude Code desktop session loaded the `aidrive` MCP server automatically (confirming the Code tab reads the user-scope registration written by `connect --client claude`). With no manual context, Claude Code called `get_project_brief`, which reported the approved handoff as **stale** because `handoff.md` had changed after approval; it re-read `handoff.md` rev 3, noticed the approved next step (script in `interview.md`) had been done in `handoff.md` instead, created `interview.md` (rev 1, attributed to the "Claude Code desktop" key) with a complete 10-minute pilot script and recording checklist, and recorded handoff `bea9c1fe` (proposed) for Codex citing the exact current versions of both files. Remaining for the full loop: owner approval in the web app, then a new Codex chat continuing from it.

### Memory M1: reviewed project knowledge — 2026-11-20

First step (M1) of the revised memory plan in [docs/research/ai-memory-findings.md](docs/research/ai-memory-findings.md).

Implementation: migration `0012_knowledge.sql` — typed immutable items (`decision`, `fact`, `constraint`, `preference`, `gotcha`, `attempt` with `failed|partial|worked`, `keystone`) with evidence pinned to an exact file version, a session event or a handoff, plus a short quote. App role has SELECT only; writes go through `app.propose_item` (files:write agents, editors, owners; idempotent per request) and `app.decide_item` (human owner only). An edited approval inserts a new approved revision and marks the proposal superseded; approving a correction supersedes the corrected item; approval is refused when cited file evidence moved. Approving a handoff creates one decision item per handoff decision (trigger), and existing approved handoffs were backfilled. API list/propose for sessions and `/agent/`, owner-only decision route; statements and quotes reject control characters. MCP 0.0.7: `search_memory`, `propose_memory`; `get_project_brief` now starts with approved knowledge (rules first, stale marked). Web: review inbox (approve, edit & approve, reject, note; approve disabled when stale) and approved-knowledge list.

Observed:
- `db-check.sh`: K01-K13 (viewer cannot propose; agent proposal with file and session evidence, normalized topic, attribution and idempotent replay; attempts need an outcome; foreign-workspace evidence refused; agents and editors cannot approve; edit-approve creates a new revision, original superseded, decisions final; approval refused after a cited file changed; corrections supersede and only approved items can be corrected; items cannot be rewritten; prefix full-text search; handoff approval creates cited decision items; deleting a session removes evidence citing it) plus all prior checks.
- `api-check.sh`: live knowledge flow (agent proposes with file and session evidence, ordinal resolved to the stable event key, proposals are not current knowledge, no agent decision route, owner edit-approve as new revision, search, stale after file change, history kept, attempt/unknown-evidence/control-character validation, outsider 404) plus all prior flows. Unit: API 19, MCP 16; fmt/Clippy clean; web lint/build pass.
- `aidrive_dev`: the backfill produced 2 approved decisions from the earlier approved handoff, both correctly flagged stale because `handoff.md` changed afterwards. Through the installed 0.0.7 binary and the Claude Code profile: tool list includes `search_memory` and `propose_memory`, the brief opens with the approved knowledge and its stale warnings, `search_memory("interview")` returned the matching decision, and `propose_memory` created a proposed gotcha attributed to the Claude Code desktop key. The web inbox listed it with owner actions; the approved list showed both decisions with their re-check warning. Nothing was approved or rejected by an assistant.
- Defect found while probing: the probe script's `\b` became a backspace character inside the proposed statement; the API now rejects control characters in statements and quotes (regression in the live flow). The existing proposal keeps the stray character and can be corrected with Edit & approve or rejected.

Not yet: M2 freshness/validity/conflict handling (disputed same-topic items, valid-from/invalid-at, strict vs historical recall), M3 brief extras, M4 visibility, M5 evaluation, M6 automatic capture.

### Memory M2: validity, conflicts and historical recall — 2026-11-20

Implementation: migration `0013_knowledge_validity.sql` — valid time (`valid_from` at approval, `invalid_at` when superseded or retired, `superseded_by`, who ended it and an optional note) next to recorded time; status `retired`; constraints tie every status to a consistent interval. Backfill derives intervals for rows created under 0012 (corrected items end at their approved successor; proposals approved with edits point at their revision and were never valid). Conflicts need no model: a proposal conflicts with approved items of the same kind and topic, except the item it explicitly corrects (`app.item_conflicts`; attempts never conflict). While such a proposal is pending, the approved items read as disputed. `app.decide_item` gained `p_resolution`: approval of a conflicting proposal is refused (AD410) unless the owner chooses `replace` (supersedes them) or `keep_both`; the slot is locked so concurrent approvals cannot slip a conflict past each other. `app.retire_item` (owner only, current approved items, once). Handoff-derived decisions are valid from the handoff approval. API: `conflicts`, `disputed_by` and validity fields on every item; `as_of` recall; `resolution` on decisions (409 `conflict_unresolved`); owner-only `POST .../knowledge/{item}/retire`. MCP 0.0.8: `search_memory(as_of)`, disputed and conflict warnings in recall and in the brief, topic guidance in `propose_memory`. Web: conflict box with Replace / Keep both (Approve disabled until chosen), disputed notice, "since" time and Retire on approved items.

Decision recorded: stale and disputed items are flagged in recall, not hidden. Hiding would make an assistant act as if no decision existed; flagging tells it to check with the owner. Tradeoff: a careless assistant can still rely on a flagged item.

Observed:
- `db-check.sh`: K14-K24 (approval opens the interval; conflict set limited to same kind and topic, attempts, untopiced items and the corrected item excluded; unresolved conflicting approval refused; unknown resolution refused; keep_both; replace supersedes every conflicting item with who/when; as-of interval query; a second correction of an already replaced item must resolve against the replacement; retire owner-only, current items only, once; handoff decisions valid from approval; app role cannot rewrite validity) and B01 (migrate a copy through 0012, seed every lifecycle shape, migrate to head, verify the backfill row by row), plus all prior checks. Mutation: disabling the resolution guard makes K16 fail.
- `api-check.sh`: live validity flow (disputed and conflict lists, 409 without resolution, 422 on unknown resolution, replace, current recall shows only the replacement, history keeps the superseded item with its bounds, `as_of` at the old item's `valid_from` returns it, `as_of` with status or an impossible date is 422, no agent retire route, owner retire once) plus all prior flows. Unit: API 19, MCP 16; fmt/Clippy clean; web lint and type check pass.
- `aidrive_dev`: 0013 applied; the two handoff decisions got `valid_from` from the handoff approval and the pending gotcha stayed undecided. Installed MCP 0.0.8 through the Claude Code profile: `search_memory(as_of="2026-11-19T12:00:00Z")` (before the handoff approval) returned nothing, a later `as_of` returned both decisions, an impossible date returned 422. Web, in a separate test space "M2 UI check": the conflict box listed the approved item, Approve stayed disabled until Replace was chosen, approval replaced the old item, Retire asked for confirmation and emptied current knowledge; no console errors. The real "AIDrive launch" inbox was not touched.

Not yet: M3 brief extras (`precheck_file`, failed attempts for touched files), M4 visibility, M5 evaluation, M6 automatic capture. Conflict detection depends on assistants choosing the same topic; untopiced items never conflict.

### Memory M3: brief v2 and precheck — 2026-11-20

Started at the owner's request before Codex's review of M1/M2 arrived; the review request now points at the fixed commits.

Implementation: API `GET .../files/{file}/precheck` (sessions and `/agent/` with files:read; read-only, evaluated under the caller's RLS, no migration). It returns the file's current version, knowledge citing any version of it (rules, unsuccessful attempts, decisions, then others), approved or proposed handoffs built on it with a stale flag, and captured sessions that mention the exact file name with whether the file changed after that mention (the `simple` parser keeps `name.ext` as one token, so "interview notes in md" does not match `interview.md`). MCP 0.0.9: `precheck_file`; `update_file` appends the approved items an edit just marked for re-checking and asks for a correction proposal; `get_project_brief` v2 orders rules, approved decisions and facts, attempts ("tried before"), what waits for the owner, the handoff rendered as text (no raw JSON; pending handoffs one line each, full in `get_latest_handoff`), what changed since this key's newest event (latest change per file by others, handoff decisions), recent activity and sessions.

Observed:
- `api-check.sh`: live precheck flow (a files:read key prechecks; order keystone, failed attempt, decision; unrelated items absent; approved items stale after the edit; stale proposed handoff; only the exact-name session event matches, with changed-since; owner session works; outsider and unknown file 404) plus all prior flows. Unit: API 19, MCP 20 (brief section order, changes-since, precheck grouping and after-edit note, handoff rendering); fmt/Clippy clean.
- Installed MCP 0.0.9 through the Claude Code profile on `aidrive_dev`: the brief opened with the two approved decisions and their re-check warning, counted 1 knowledge proposal and 3 handoffs waiting for the owner, rendered the stale approved handoff with its source versions and the pending ones as single lines, and reported nothing changed since this key's last event; the whole brief measured about 4,000 characters. `precheck_file(handoff.md)` listed both decisions that rely on it, the approved handoff as built on an older version, a proposed handoff on the current one, and the Codex session that mentioned `handoff.md` before it changed. `precheck_file(interview.md)` reported no knowledge and two proposed handoffs.

Not yet: the after-edit note was exercised by unit tests only (no real file was edited for it); no precheck view in the web app; session mentions are matched by name, so a same-named repository file can match.

### Memory M4: visibility — 2026-11-21

Started at the owner's request while Codex's review of M1-M3 is pending (Codex hit its usage limit). It answers the "user-owned AI workspace" promise in the founder's DriveX strategy notes: who can see what an assistant captured or learned.

Implementation: migration `0015_visibility.sql` (numbered 0015 because a separate, uncommitted local-trust change in the `aidrive-local-trust` worktree already uses 0014 and is applied to `aidrive_dev`). `private` means only the creator and the assistants acting for that person (an agent's requests run as its issuing owner); workspace owners do not see other people's private records. `team` means every reader. Captured sessions default to private (existing sessions became private); knowledge defaults to team. Row-level security on sessions, session events, knowledge items and knowledge evidence uses `app.can_see`. Only the human creator changes a session's visibility (`app.set_session_visibility`); re-capture never merges into another person's session (409), only the creator's assistants add events, and the creator or a workspace owner deletes a session. Evidence may cite only sessions the proposer can see. Knowledge visibility is chosen when proposing and never changes; private items are decided and retired by their creator (to anyone else they do not exist), cannot correct team items or be corrected by them, and conflicts never cross visibility. API: `visibility` and `mine` on sessions and knowledge, `POST .../sessions/{session}/visibility`, `visibility` on proposals. MCP 0.0.10 (source only, see below): `propose_memory(visibility)`, private items labelled, session visibility in the brief. Web: session visibility with Share with team / Make private for the creator, creator delete, private label and creator-only decisions for private knowledge.

Observed:
- `db-check.sh`: R01-R14 (private sessions invisible to other members and to the workspace owner, also in search; the owner's assistants read but cannot share; other members cannot share, join, append to or delete someone's session; only private/team accepted; sharing reveals the session and its events to the team; private knowledge and its evidence invisible to the workspace owner, who cannot decide or retire it; creator approval; conflicts inside one person's private items only; no private correction of team items and no team correction of private items; assistants cannot cite private sessions they cannot see; creator retire and delete; the app role cannot rewrite visibility) and B02 (existing sessions become private, existing knowledge stays team), plus all prior checks. The session checks S01-S09 now share their test session first, since viewers no longer see private sessions. Mutation: reverting the sessions read policy to "any viewer" makes R01 fail.
- `api-check.sh`: live visibility flow (private by default, hidden from another member and search, assistant has no sharing route, non-creator 404, invalid value 422, sharing makes it visible and searchable, merge into another person's session 409, workspace owner does not see an editor's private session, private proposal outside the owner's inbox, owner decision 404, creator approval, owner's assistant does not see it, creator delete) plus all prior flows. Unit: API 19, MCP 20; fmt/Clippy clean; web lint and type check pass.

Not yet: `restricted` visibility (named people or assistants) is not built. Dev rollout after the local-trust merge is recorded below.

### Local trust: an assistant connects itself — 2026-11-21

Founder requirement (chat, 2026-11-21): for the open-source local install the user only tells their assistant "connect"; the assistant does the rest, with no sign-in, password or approval click.

Decision: **local trust**, opt-in with `AIDRIVE_LOCAL_TRUST=1` and only while the API is bound to loopback. On such an install the computer's account is the owner. `connect` claims an owner **device key** once (`%APPDATA%\AIDrive\device.json`, only its SHA-256 stored), then uses it to create or reuse the space named after the project folder and to issue the assistant's ordinary agent grant; `aidrive-mcp-server open` uses it to mint a one-time, two-minute web sign-in link. Without local trust, or when the API answers anything but 201, `connect` falls back to the browser pairing of day 5a. Tradeoff: any program running as this OS user can do what the owner can. That is already true locally (it can read the agent profiles and reach the WSL database), so the boundary does not move; hosted installs keep owner approval. Hypothesis, not yet checked against primary sources: this matches how local developer tools trust the logged-in user (a token file in the user profile). Research step still owed under AGENTS.md.

Safeguards: claim works only with zero accounts (creates `owner@local.invalid`) or exactly one (that person); more than one is refused as not a single-person install. Local routes refuse requests that carry `Origin` or `Sec-Fetch-Site` and any non-loopback `Host`, so a web page (including a DNS-rebound one) cannot pick its own device secret. Device keys have their own prefix and are not agent keys; at most 20 per owner; login codes are single use.

Implementation: migration `0014_local_trust.sql` (`owner_devices`, `local_login_codes`, `app.claim_local_device`, `app.resolve_owner_device`, `app.issue_local_login`, `app.redeem_local_login`; app role has no table access). API module `local.rs`: `POST /agent/local/claim`, `/agent/local/connect`, `/agent/local/login`, `GET /local/login/{code}` (opened through the web app's `/api` proxy so the cookie lands on the web origin). MCP: `connect --space NAME` (default: the current folder name), `--browser-approval` to force the old flow, new `open` command.

Observed:
- `api-check.sh`: new live local-trust flow on its own empty database (off without the flag; Origin and rebound Host refused; first claim creates the local owner, a reused hash conflicts, a second device maps to the same owner; unknown device and agent keys refused; connect creates "Side project", the issued key reads it, a second assistant joins the same space; login link signs in once and a second use is refused; a second account makes claim 409; routes 404 with the flag off) plus all prior flows. Unit: API 22, MCP 16; fmt and Clippy clean.
- `aidrive_dev` (backup taken first, `target/backups/aidrive_dev-before-0014.dump`): 0014 applied; this branch's API on 127.0.0.1:8081 with local trust. From the Side project project folder, `connect --client claude --api http://127.0.0.1:8081 --no-register` printed "Connected on this computer without approval (created the space; owner owner@aidrive.test). Space "Side project" (files:write)" with no browser and no click. The resulting key read the space through the main API on 8080, and two files were created in it with that key.

Not yet: the main API (8080) runs without this code, so `open` and further zero-touch connects need it merged and restarted with `AIDRIVE_LOCAL_TRUST=1`; device revocation in the web app; a per-IP limit on claims; the research step above; Codex not reconnected.

### Local trust merged with M3/M4 — 2026-11-21

At the owner's request ("You merge it"), the local-trust work was committed unchanged on its branch (`3104242`, attributed to the parallel session that wrote it) and merged into `main` (`68b27ce`). Conflicts: `apps/api/src/main.rs` (both route sets kept) and `ROADMAP.md` (both sections kept). The migration-number collision was settled by keeping local trust as 0014 (already applied to `aidrive_dev`) and visibility as 0015. MCP server 0.0.11 combines the local `open` command with M4.

Review before merging: local routes answer only with `AIDRIVE_LOCAL_TRUST=1` and a loopback bind, refuse requests that carry `Origin` or `Sec-Fetch-Site` or a non-loopback `Host`, store device keys as SHA-256 only, and give device keys their own prefix. Remaining limits: any process running on this computer, including under another OS account that can reach loopback, can claim the owner while exactly one account exists; device revocation has no UI; `connect` writes one profile per client, so connecting from a second project re-binds that client (this is how the Claude profile moved to the Side project space at 18:41, with the previous profile kept as `claude.aidrive-launch.json`).

Observed:
- Merged tree: fmt/Clippy clean; API unit 22, MCP 20; `db-check.sh` passes; `api-check.sh` passes including the local-trust, precheck and visibility live flows.
- `aidrive_dev`: the migrator accepted the existing 0014 and applied 0015; the imported Codex session became private to its creator (the owner) and existing knowledge stayed team. The dev API now runs the merged build with `AIDRIVE_LOCAL_TRUST=1`; a claim carrying a browser `Origin` was refused with 403. Installed MCP 0.0.11 under the Claude Code desktop key of "AIDrive launch": `propose_memory` exposes `visibility`, the brief lists the session as private, and `search_sessions` still finds it for the owner's assistant. Web: the session shows "Private: only you and your assistants" with Share with team and Delete; the only console error was the 401 of the session check before signing in. Nothing was shared, approved or deleted.

Not yet: per-project client profiles; device revocation UI; the primary-source research step for the local-trust model (owed under AGENTS.md); Codex has not reviewed M1-M4 or local trust.

### Memory M5: evaluation harness, development set and lexical baseline — 2026-11-21

Scope against the locked protocol in docs/PRODUCT.md: this builds the 60-case development set and the retrieval harness and records a baseline. The locked 100-case held-out set (a different project history), answer-generation evaluation (needs provider budget and frozen models) and human review are not done.

Implementation: `eval/dev/tutorbook.json` is a synthetic project history for a fictional tutoring app, written for this set (not the founder's data), in English and Armenian: 3 files with versions, 4 visible sessions (59 events), an editor's private session, a session in another workspace, a deleted session, 33 knowledge proposals (31 approved: 29 current and 2 later superseded; 2 left pending), a stale item and a disputed item. Its 60 cases: session recall 20, knowledge recall 16, changed facts 6, temporal (`as_of`) 4, stale/disputed flags 4, abstention 6, permission 4. `apps/api/src/eval_tests.rs` seeds it through the real API on the private cluster, runs every case as the owner's assistant (`search_sessions` / `search_memory` paths, limit 10) and prints recall@10 by language and category, an exact random-k control (10/N for a pool of N), flag correctness, abstention and violation counts. Only invariants are asserted: zero forbidden sources (private, other workspace, deleted, unapproved), zero superseded items as current, zero later items for an earlier `as_of`. It runs inside `api-check.sh`.

Defects the evaluation found, fixed with regression checks:
- Query construction paired quotes across neighbouring words: after any 1-2 character word (`ի`, `է`, `we`, `9`) the next word became `':*word'` and never matched (e.g. "PayLink-ի միջնորդավճար" returned nothing). Both session and knowledge search now share `sessions::MATCH_QUERY`, which takes every quoted lexeme in order before adding prefixes. Knowledge search also ranks with the same any-term query (it ranked with an all-terms query, which scored partial matches 0).
- `-term` exclusions were turned into `| !term` by the any-term rewrite and matched almost everything; they now stay ANDed (`pricing -seat` excludes seat). Limit: with several positive words the exclusion binds only to its neighbour.
- The private test cluster used `--locale=C`, which does not lower-case Armenian (or Cyrillic) capitals, so sentence-initial words never matched lowercase queries. The dev database already uses C.UTF-8. The test cluster now uses C.UTF-8, and API readiness fails (`schema: failed`) when `lower('Բ') <> 'բ'`, so a database that cannot fold Armenian is not silently served.

Observed (private cluster, C.UTF-8, after the fixes): recall@10 0.82 vs random-k 0.32 (51 answerable cases); English 0.84, Armenian 0.81; session recall 0.75, knowledge recall 0.94, changed facts 0.67, temporal 1.00, flags 3/4; abstention 0/6 (57 hits returned across the 6 unanswerable cases); forbidden 0, superseded-as-current 0, temporal violations 0. Before the fixes, on the C-locale cluster: 0.75 overall, Armenian 0.65. Targets in docs/PRODUCT.md (>=0.95 overall, >=0.90 per language/category, >=95% abstention) are not met.

What the misses say (decision input): (1) answers that do not repeat the question's words (4 session cases: the question event matches, the answer event does not); (2) Armenian inflection when the query form is longer than the stored form (`ինտերֆեյսի` vs `ինտերֆեյսը`, `բետայի` vs `բետան`); (3) paraphrase and numbers ("9 dollars" vs "12 USD"); (4) cross-language (an English question about an Armenian item); (5) no abstention at all, because any-term matching without stopwords always finds something. (1), (2) and (5) look fixable lexically (pair each question with its answer, a light Armenian suffix normaliser, stopwords plus all-terms-first ranking with a coverage threshold); (3) and (4) need semantic retrieval (multilingual embeddings), which is a model, cost and privacy decision for the founder. Measure each change on this development set; build the held-out set before claiming any target.

### Memory M5b: query planning measured on the development set — 2026-11-21

Implementation: `apps/api/src/search.rs` plans plain questions for session and knowledge search: lower-case, split on non-alphanumerics, drop English and Armenian stopwords, remove one Armenian case/article/plural ending (stem keeps 4+ letters) or an English -ing/-ed/-es/-s, match 3+ character terms as prefixes, rank sources by how many terms they match, then by `ts_rank_cd`. Session search also returns the assistant answer that follows a matched user question. Quoted phrases and `-term` keep the websearch path from M5. Unit tests cover the planner; all prior live flows still pass.

Measured on eval/dev (private cluster, C.UTF-8):

| Setting | recall@10 | EN | HY | session | knowledge | changed facts | flags | abstention | violations |
|---|---|---|---|---|---|---|---|---|---|
| M5 baseline | 0.82 | 0.84 | 0.81 | 0.75 | 0.94 | 0.67 | 3/4 | 0/6 | 0 |
| planner, half of the terms required | 0.85 | 0.88 | 0.83 | 0.88 | 0.94 | 0.67 | 3/4 | 6/6 | 0 |
| planner, one term required (kept) | 0.95 | 0.92 | 0.98 | 0.97 | 1.00 | 0.83 | 3/4 | 5/6 | 0 |

Random-k control 0.32 throughout. "Half of the terms" dropped answers that match one distinctive word out of three or four; with stopwords removed, five of six unanswerable questions already match nothing, so one term is kept.

Remaining misses: a paraphrase with a number ("Do we still charge 9 dollars?" vs "12 USD"), a cross-language question (English question, Armenian item), one of two expected events for "which payment system did we choose", and one unanswerable question that matches the generic word "use". The first two need semantic retrieval.

Caution: these numbers are tuned on the development set. Against docs/PRODUCT.md, changed facts (0.83), flags (0.75) and abstention (83%) are below target even here, and no claim can be made before the locked held-out set (a different project history) is built and run.

### Per-project profiles — 2026-11-21

Problem observed today: profiles were one per client, so connecting Claude Code from the Side project folder re-bound every Claude session on this computer to the Side project space.

Implementation (MCP server 0.0.12): `connect` saves a second space as `<client>-<space>` instead of overwriting a default that points elsewhere, and records `folder -> profile` per client in `%APPDATA%\AIDrive\profiles\bindings.json`; `connect --project <folder>` sets the folder explicitly (the space name defaults to its name). `serve --profile <client>` resolves the nearest enclosing bound folder of its working directory, otherwise the client's default. New commands `profiles` (lists profiles and bindings, never keys) and `bind`. `get_workspace` adds `profile`, `profile_bound_folder` and `process_cwd`. The client registration command is unchanged.

Observed: MCP unit tests 22 (nearest-folder resolution, per-client bindings, Windows path normalisation, naming that never re-binds an existing default); fmt/Clippy clean. On this computer, with the owner's existing profiles: the saved AIDrive launch profile (`claude.aidrive-launch.json`, an invalid profile name) was renamed to `claude-aidrive-launch.json` with unchanged content and bound to `...\bu`; `serve --profile claude` started from `bu\aidrive\apps\web` and from the `aidrive-local-trust` worktree used "AIDrive launch", and from `<documents folder>` kept the default "Side project".

Not yet: whether Claude Code and Codex start MCP servers in the project folder is unverified; a new session's `get_workspace` (`process_cwd`) will show it. If a client starts servers elsewhere, sessions fall back to the default profile, as before.

Observed 2026-11-21 (T1 session): a new Claude Code desktop session started the installed server with `process_cwd` = `...\bu\aidrive` (the main project folder, not the session's worktree under `.claude\worktrees\...`), resolved profile `claude-aidrive-launch` through the binding on `...\bu`, and `get_workspace` reported "AIDrive launch". So Claude Code desktop starts MCP servers in the project folder. Codex is still unverified.

### Messages between people and assistants — 2026-11-22

Problem observed: assistants coordinated through files (`message-for-codex.md`, `message-for-claude.md`). Each edit made a handoff that cited the file stale (it could then not be approved), and nothing showed who had read what.

Implementation: migration `0016_messages.sql`: messages to all assistants, to the owner, or to one active assistant key (by name or id), replies in threads, up to 10 cited file versions, per-reader read receipts (each key, or the person); written only through `app.send_message` (editors/owners and files:write keys, idempotent per request) and `app.mark_messages_read`, read through `app.workspace_messages` (definer, so names are visible; inbox and unread filters); `message.sent` joins the activity timeline. Every reader of the space sees every message, so the owner can supervise; there is no private messaging. API `GET/POST .../messages`, `GET .../messages/{id}`, `POST .../messages/read` (session and agent routes). MCP 0.0.13: `send_message`, `get_messages` (unread for me, then marked read; or a thread; or recent), "Messages for you" in the brief. Web: Messages panel (list, send, reply; unread messages to the owner highlighted and marked read when shown), `message.sent` in the activity panel.

Observed: db tests G01-G09 (viewer cannot send; attribution, cited version, idempotent replay; changed replay, revoked recipient and foreign-workspace source refused; replies join the thread; an assistant's inbox holds broadcasts it did not send and direct messages to its key; read receipts per reader; a files:read key reads broadcasts but not other keys' direct messages and cannot send; the owner sees all and the activity entry; no direct writes; outsiders see nothing). Live API messages flow passes, and all prior flows still pass (eval recall@10 0.95). On `aidrive_dev` (migrated to 0016; API restarted with this build): through the installed MCP 0.0.13 under the Claude Code desktop key of "AIDrive launch", `send_message` posted a real broadcast asking Codex to use messages and review `git diff cbf104d~1 main`; `get_messages(all)` listed it; the sender's own inbox stayed empty; activity showed "sent a message". Web, signed in as the local test owner: the message appeared in "AIDrive launch"; sending from the UI in the test space "M2 UI check" listed the new message and cleared the form.

Coordination risk: the uncommitted T1 work (worktree `confident-bassi-cec44c`, migration `0017_review_by_exception.sql`) also replaces `app.workspace_activity`. Merged after 0016 as written, it would drop `message.sent`; db test G08 fails in that case, so whoever merges T1 must keep both unions.

Not yet: notifications (an assistant sees messages only when it calls `get_messages` or the brief); message deletion or editing; a Codex reply has not been observed yet.

### T1 approval by exception: decision — 2026-11-21

Problem (observed in `aidrive_dev`): every handoff and knowledge proposal waits for the owner; on 2026-11-21 "AIDrive launch" had 6 proposed handoffs (the oldest, `bea9c1fe`, from 2026-11-20) and 1 knowledge proposal, while the only approved handoff was stale. The adviser review (ADVISER.md, Phase 2, verdict PIVOT) names approval friction as the biggest risk.

Decision: per-space review mode. `exceptions` (default for spaces created from now on through the API or local connect) accepts a proposal at once when nothing about it needs the owner; `strict` keeps today's behavior. Existing spaces stay `strict` until their owner switches, so no record in "AIDrive launch" changes by migration. Only the human owner changes the mode; assistants cannot.

Accepted automatically only when all hold: the mode is `exceptions`; at least one piece of evidence (a knowledge item) or one cited file version (a handoff), and every cited file is still at the cited version; for knowledge also not a keystone, not a correction of an approved item (it would end that item), and `app.item_conflicts` is empty. Everything else waits in the inbox as an exception: conflicts, stale or missing evidence, keystones, corrections (the owner's retire and delete stay owner actions). Auto-accepted records carry `auto_accepted = true`, are attributed to the proposer and labelled "auto-accepted, not checked by the owner" in the brief, `search_memory`, the API and the web app. The owner can Keep (records `confirmed_by`, which makes it owner-checked) or Undo in one action: a knowledge item is retired (it was current knowledge for a while, so `as_of` keeps it), a handoff becomes rejected, its derived decisions are retired and the handoff it replaced becomes current again. Handoffs keep "one approved handoff at a time"; the parallel-stream competition is not fixed here.

Tradeoff: "approved" no longer means "the owner checked it". An assistant's claim with current evidence and no conflict becomes knowledge the next assistant reads, and a wrong one stays until the owner notices it in "recently auto-accepted". This weakens the governance promise in PRODUCT.md ("the owner approves lasting project facts") to "the owner is asked about exceptions and can undo the rest"; the label keeps the two guarantees apart, and strict mode keeps the old one. Rejected alternatives: batch approval (still daily friction), confidence scores from a model (no model in MVP, not auditable), agent-majority approval (PRODUCT.md: two agents agreeing does not make it true).

Sources: ADVISER.md Phase 1-2 (2026-11-21; peers: caura auto-writes memories and stages only agent-written skills in an operator inbox; powercontext reviews every item; Letta has no human approval by default) and docs/research/ai-memory-findings.md (2026-11-20). Fresh primary-source web research was not repeated in this session (owner rule: external HTTP needs a separate yes). Hypothesis, not observed: the undo rate stays low enough that owners trust auto-acceptance. Metric: `GET .../review/metrics?days=N` (owner approvals, auto-accepted, kept, undone per day, from the records themselves).

### T1 approval by exception: implementation and evidence — 2026-11-22

Implementation: migration `0017_review_by_exception.sql`, rebased onto `main` after `0016_messages.sql`; its `app.workspace_activity` keeps 0016's `message.sent` union (G08 still passes). `workspace_settings.review_mode` (`exceptions` | `strict`; no row = strict), changed only through `app.set_review_mode` by the human owner. `auto_accepted`, `confirmed_by/at` on knowledge items and handoffs, plus `replaced` and `undone_by/at` on handoffs. `app.propose_item` and `app.record_handoff` call `app.auto_accept_item` / `app.auto_accept_handoff` (internal, not granted) for new requests only; the item path takes the same slot lock as `decide_item`, so two proposals for one topic cannot both pass as conflict-free. Decisions of an auto-accepted handoff are auto-accepted items. Owner actions: `app.confirm_item` / `app.undo_item` (retire, history kept), `app.confirm_handoff` (also checks its decisions) / `app.undo_handoff` (rejects, retires its unchecked decisions, restores the handoff it replaced if that one is still superseded and nothing else is current); all refuse agents and return not-found to non-members. `app.decide_handoff` now also records `replaced`. Timeline: `handoff.auto_accepted|kept|undone` and `knowledge.proposed|approved|auto_accepted|rejected|kept|undone|retired` (visible items only; handoff-derived decisions count with their handoff). Metric: `app.review_metrics(workspace, days)`.

API: `POST /workspaces` takes `review` (default `exceptions`); local `connect` sets `exceptions` on the space it creates. `GET .../review` (sessions and `/agent/`: mode plus auto-accepted records not yet kept or undone), owner-only `POST .../review {mode}`, `GET .../review/metrics?days=1..90` (per day owner approvals, auto-accepted, kept, undone, owner rejections; totals, owner decisions per day, undo rate), owner-only `POST .../knowledge/{item}/keep|undo` and `.../handoffs/{h}/keep|undo`. Items and handoffs carry `auto_accepted`, `owner_checked`, `confirmed_at` (`undone_at`, `replaced` on handoffs) and `review_reasons` on pending ones (`strict_review`, `keystone`, `correction`, `conflict`, `stale_evidence`, `no_evidence`, `stale_sources`, `no_sources`, `exception_cleared`). MCP 0.0.14 (source only, not installed): `search_memory` and the brief label "auto-accepted, not checked by the owner"; the current handoff reads "CURRENT HANDOFF, AUTO-ACCEPTED ... unconfirmed by the owner"; the brief states the space's review mode and how many auto-accepted records are unchecked; `propose_memory` and `record_handoff` results open with "Auto-accepted: ..." or "Waiting for the owner (reasons)"; tool descriptions and server instructions explain the rule. Web: a Review panel (mode with confirmation, 7-day metric, "Recently auto-accepted" with Keep and Undo), the knowledge inbox renamed "Needs your decision" with the reason per item, auto-accepted labels on approved knowledge and handoffs, new timeline wording. PRODUCT.md's first-release approval sentence and the "do not silently auto-approve" rule were rewritten to match.

Observed:
- `db-check.sh`: E01-E21 in `db/tests/knowledge_review.sql` (no setting = strict; assistants and editors cannot change the mode; unknown mode refused; app role cannot write settings or flags; clean proposal auto-accepted, flagged, attributed to its proposer; conflict, keystone, correction and unevidenced claim wait; older cited version waits while the same slot with current evidence passes; private proposals follow the rule for their creator; assistants cannot keep or undo; editors cannot undo team knowledge; undo retires with history, keep records the owner check; only unchecked auto-accepted items, once; handoffs with current sources auto-accepted one at a time, stale and unsourced ones wait, derived decisions flagged; undo restores the replaced handoff and retires its decisions; keep covers derived decisions; owner approval stays owner-approved and records what it replaced; timeline kinds; metrics per day; another person's private items left out of metrics and timeline; back to strict) plus all prior checks unchanged.
- `api-check.sh`: new live flow `review_by_exception_flow_against_live_database` (new space defaults to exceptions, invalid mode 422, clean proposal approved + `auto_accepted`, conflict and keystone with reasons, recall labelled, auto handoff current, unsourced handoff waits, review lists one item and the handoff, no assistant mode or undo route, outsider undo 404, owner undo once then 409, keep marks owner-checked, metrics 3 days with undo rate 0.5, `days=0` 422, timeline kinds, switch to strict and proposals wait again) plus all prior flows; the memory evaluation still reports recall@10 0.95 (its seed space is strict). Existing flows that assert pending states create their spaces with `"review":"strict"`. Unit: API 27, MCP 24; fmt and Clippy (`-D warnings`, api, mcp-server, db-tool) clean; web lint, type check and production build pass. Re-run after rebasing onto `1f9963f`: all of the above plus the messages db tests (G08 included) and the messages live flow pass.
- Web, against a separate database `aidrive_t1` (API on 127.0.0.1:8082, web dev on :3002, test account, space "T1 UI check"; `aidrive_dev` and "AIDrive launch" untouched): inbox showed only the rule, the unevidenced gotcha and the conflicting fact, each with its reason; Review listed the auto-accepted fact and handoff; Undo on the fact and Keep on the handoff emptied the list, the metric read 2 auto-accepted, 1 kept, 1 undone (50%), the conflicting proposal's reason changed to "nothing blocks it now", and strict/exception switching worked both ways.
- Defects found while testing, fixed: a bound-but-unused note parameter written as `... OR $3 IS NULL` was constant-folded by PostgreSQL so the keep call never ran (now a sub-select); undo/keep by a non-member answered 403 instead of 404.

Not yet / known limits:
- "AIDrive launch" and every space created before 0017 stay strict until the owner switches them in the Review panel.
- Parallel handoff streams still compete for the one current handoff; with auto-acceptance the newest wins immediately (the replaced one stays in the list and comes back on undo).
- Auto-acceptance trusts the assistant's own choice of topic: a contradicting statement under a different topic (or none) is not a conflict and is accepted. This is the main hole in the weaker guarantee and is what the undo rate should reveal.
- Metrics count records the caller can see; no per-assistant breakdown yet.

Dev rollout (owner: "Yes, merge", then "Yes, roll out", 2026-11-22): `main` fast-forwarded to `bccb934`; backup `target/backups/aidrive_dev-before-0017.dump`; the migrator moved `aidrive_dev` from 16 to 17 with no `workspace_settings` rows, so every existing space is strict; `aidrive-api` rebuilt from `main` and restarted through `aidrive-api.service` (`/readyz` ready, `/review` routed); MCP 0.0.14 installed (old binary kept as `aidrive-mcp-server.retired-*.exe`), and a fresh `serve --profile claude` from the project folder reported version 0.0.14 and a brief for "AIDrive launch" saying "This space uses strict review". Running Claude Code and Codex sessions keep the old server process until they restart. Nothing was approved, kept or undone in the real space.
- Incident during the UI check (2026-11-21): a too-broad `pkill -f debug/aidrive-api` in this session also stopped the dev API on 8080; it was restarted from the main checkout with the checkpoint command and `AIDRIVE_LOCAL_TRUST=1`. Separate API builds now use their own target directory. The dev API on 8080 is kept up by the systemd unit `aidrive-api.service` (it restarts the binary in `target/wsl/debug`), so a rollout is: build `aidrive-api` from `main`, then `systemctl restart aidrive-api`.

### M6 automatic session state (A2) — 2026-11-22

Owner requirement: AIDrive must not wait for a session to send a handoff; a session cut off by a usage limit must leave everything for the next assistant, including sub-sessions working in parallel.

Implementation (MCP server 0.0.15): `capture --client claude --auto` reads Claude Code hook input, resolves the space from the folder binding of the session's working directory (unbound folders are never captured), and uploads only events not yet sent (`%APPDATA%\AIDrive\capture-cursor.json`); it always exits 0 so a hook can never block a session. `brief --auto` prints the space's brief for a SessionStart hook (Claude Code adds hook output to the session context). `install-hooks [--remove]` writes SessionStart (brief) and Stop / PreCompact / SessionEnd (capture) hooks into `~/.claude/settings.json`, idempotently, keeping other hooks, with a one-time backup. The brief section "Where each recent session left off" shows, for the three most recent captured sessions, the last request, the last answer and recent tool notes, deterministically from captured events (no model).

Observed: MCP unit tests 27 (hook install idempotent, keeps other hooks, removes cleanly; auto mode parsing; left-off rendering). On this computer: hooks installed (backup taken, the existing PostToolUse hook kept); backfill captured the four Claude Code sessions in AIDrive folders: main checkout (2 events), T1 worktree (353), T2 (155), T3 (94); a second run reported "up to date"; a transcript from `<documents folder>` (unbound) was skipped; malformed input exited 0. `brief --auto` from the aidrive folder printed the brief, which showed that T1 was cut off by its usage limit while being asked to build this feature; from an unbound folder it printed nothing.

Not yet: Codex hooks (its `notify` is used by the Codex app); sessions started outside bound folders (for example orchestrator sessions in `<documents folder>`) are not captured; a fresh Claude Code session with the installed hooks has not been observed yet; an automatic handoff draft derived from the capture is not built (the brief's "left off" view replaces it for now).

### M6b session records, handoff drafts, consent and Codex hooks; A3 binding warning; A4 release audit — 2026-11-22

Owner refinement of A2 (relayed by the adviser session): a session never has to send its handoff; AIDrive writes what happened the same way every time, from the harness, not from the model.

Implementation (MCP server 0.0.16, source; not installed):
- Session record (`apps/mcp-server/src/record.rs`): built deterministically from the captured, already redacted transcript, no model. The sections are always the same: request, latest request, files changed by the assistant (from Edit/Write/NotebookEdit/apply_patch tool calls), working tree (`git` branch, HEAD and `diff --stat HEAD` of the session's folder, redacted), commands run (shell tool calls; output is not captured), decisions the assistant stated (matched by wording, marked unconfirmed), what is left (matching lines of the last answer). The header says it is derived and not approved knowledge, and names the captured session as the source. Quoted transcript text cannot open a heading or list of its own. It is saved in the space as `session-auto-<folder>-<date>-<id>.md`: rewritten at most every 15 minutes while new events arrive (so a session cut off by a usage limit still leaves a recent record), and always on PreCompact and SessionEnd; if someone edited the file, the next write builds on their version.
- Handoff draft: on SessionEnd the hook records a handoff whose summary says it was derived from the transcript, with no proposed decisions, the "what is left" line as next step, and the record's exact file version as source. Under review by exception it is auto-accepted when its source is current; in a strict space (such as "AIDrive launch") it waits for the owner. Once per record version.
- Consent per project: `capture-consent.json` (client -> bound folder -> on/off). `connect --capture` switches it on and installs the client's hooks; `connect --no-capture` and `capture-consent --client C [--project DIR] off` switch it off; the SessionStart brief and the capture both require it. A computer that had hooks before this file existed starts with every bound folder on (binding was the consent until now). Retention is printed when capture is switched on: sessions stay until deleted in the web app, private to the person by default; only messages and short tool notes leave the computer, after local redaction.
- Codex: checked locally, not assumed. Codex CLI 0.164.0 reads `~/.codex/hooks.json` in the Claude Code shape (this computer has a PostToolUse hook there) and keeps trust hashes per hook in `config.toml`; the hook state on this computer shows `session_start`, `stop`, `post_tool_use` and `permission_request`. `install-hooks --client codex` (and `connect --client codex --capture`) writes only SessionStart (brief) and Stop (capture). The capture takes `transcript_path` from the hook input when present, otherwise the newest rollout under `~/.codex/sessions` whose working directory is inside the bound folder, parsed by the existing Codex importer.
- A3 (wrong space and uncaptured sessions, including the issue reported from the Side-project space that sessions there were not captured): when a session's folder is outside every bound folder (default profile), `get_workspace` returns `warning` and the brief opens with "Which space this is": the folder, the default profile, the space it works in, and the exact `bind` / `connect --project` commands. Whenever a session is not captured (unbound folder, or bound with capture switched off), the SessionStart hook prints "AIDrive: this session is NOT captured automatically" with the fix, and `get_workspace` returns `captured_automatically: false` with the same text. `bind` (CLI) and the new MCP tool `bind_folder` switch capture on for the folder they bind, so a folder bound before the consent list existed cannot be skipped silently; `bind_folder` refuses unless `confirm_owner_said_yes` is true, and its description says to call it only after the owner said yes in the conversation. Codex and other uncaptured sessions get a rule in the server instructions: write a session note with `create_file` and call `record_handoff` before stopping; existing Codex rollouts are imported with `capture --client codex --project DIR`. `--help` lists every command.
- A4: history scan of all 40+ commits (`git log --all -p`): no real keys, tokens or passwords; matches are test fixtures (`aidrive_agent_0123…`, `sk-abcdef…`, a dummy PEM in a redaction test, private-cluster passwords generated at run time) and the documented dev-only database password `aidrive` in `.env.example`, which is the only tracked env file. Local identifiers were in the current tree and are replaced: the owner's user path in ROADMAP and another project's name in tests and ROADMAP. They remain in the existing history; since nothing has been pushed, the owner can still publish from a fresh root if wanted. `ADVISER.md` holds pricing, kill criteria and hardware notes, and every commit carries the author's email; whether both go public is the owner's call. `.gitignore` now covers `/.claude/`, `mitm_mcp_traffic.db`, `*.dump` and `tsconfig.tsbuildinfo`. CI: `--locked` everywhere, a 1.88 MSRV job (the declared `rust-version` was never checked), web type check, timeouts and concurrency; it runs the same `db-check.sh` / `api-check.sh` as local work. CI has not run remotely (nothing is pushed).

Observed:
- MCP unit tests 37 (uncaptured-session warning and usage, untrusted text cannot forge brief sections, record sections and determinism, quoted text cannot forge a heading, empty sessions, stable file names, git state, record timing, consent from existing bindings and off switch, `--capture`/`--no-capture` parsing, Codex hooks only SessionStart/Stop and removed cleanly, the binding warning); Clippy `-D warnings` and fmt clean.
- T2 finding 1 (unapproved text could forge brief sections such as "## Rules (must follow)") is fixed: statements, summaries, decisions, next steps, notes, labels, topics, message subjects and bodies and captured last request/answer are rendered on one line without leading Markdown markers.
- Live, from an unbound scratch folder with the 0.0.16 debug build: the SessionStart output was the NOT-captured warning with the bind command; `get_workspace` returned `captured_automatically: false` and both warnings (it was on the default profile, the Side-project space); `bind_folder` without confirmation refused; with confirmation it bound the folder to the test profile, switched capture on, and the next SessionStart printed the brief of "T1 smoke".
- Live, with the 0.0.16 debug build against the dev API, in the test space "T1 smoke" (review by exception, bound to a scratch folder, connected through local trust): a SessionEnd hook input with a three-line Claude transcript (containing `password=hunter2`) captured 4 events, wrote `session-auto-t1-smoke-2026-11-22-hooktest.md` (all sections present, the password shown as `[REDACTED]`) and recorded a handoff that was auto-accepted with the record's version as source; a second run reported "up to date" with no new record or handoff; with capture switched off for the folder another session was not captured; switched on again afterwards.
- T1 smoke through the installed 0.0.14 client before this work: a clean proposal was auto-accepted, a conflicting one waited ("Waiting for the owner (conflict)"), a handoff with current sources was auto-accepted, and `search_memory` and the brief labelled both as not checked by the owner.

Not yet: 0.0.16 is not installed (the hooks on this computer still run 0.0.15, which captures without the consent check and writes no record); a real Codex session through the hook was not observed, and whether Codex passes `transcript_path` or adds SessionStart output to the model's context is unverified; the optional local-model polish of the record is not built (the record is deterministic only); command results are not in the record because tool output never leaves the computer; Codex rollout import is manual (`capture --client codex --project DIR`).

### A5 no sign-in on a local install — 2026-11-23

Owner requirement (relayed by the adviser session, 2026-11-22): "delete login pass check local no one need". On a local install, opening the web app must show the owner's spaces with no form, no password and no prompt. Hosted installs keep email and password.

Decision: with local trust (`AIDRIVE_LOCAL_TRUST=1` and a loopback bind) the web app's own page on this computer opens the owner's session. Tradeoff: any program running as this OS user that can make a same-origin request from a loopback page is the owner. That is already true under local trust (such a program can read the device key and the agent profiles, or reach the database), so the boundary does not move. What a web page elsewhere could do is closed by protections the user never sees: every host the request names (the browser's Host, passed by the web proxy as `X-Forwarded-Host`, and the API's Host) must be `localhost`, `127.0.0.1` or `[::1]`, so a DNS-rebound page is refused; the request must be same-origin by Fetch Metadata (or carry a loopback Origin), and any Origin must be loopback; the session cookie stays `__Host-`, `HttpOnly`, `Secure`, `SameSite=Strict`; nothing answers unless local trust is on, and more than one account means no automatic sign-in.

Implementation: migration `0019_local_session.sql` (0019 because another session holds an uncommitted `0018_agent_key_lifetime.sql`) (`app.open_local_session`: same single-person rule as `claim_local_device`; no users creates the local owner; starts a 7-day web session). API `POST /local/session` (sets the cookie) and `GET /local/mode` (`{"local": bool}`, nothing else). Web: on a 401 the page asks `/api/local/mode`; in local mode it opens the session itself and shows "On this computer as …" with no sign-in form, Create account or Sign out; with two accounts or a refused request it says why instead of showing a form. `aidrive-mcp-server open` (one-time link) still works.

Observed:
- Unit: DNS-rebound `X-Forwarded-Host`, cross-site Fetch Metadata, a foreign Origin and a plain request without browser headers are refused; the proxied same-origin request passes; loopback origin parsing.
- `api-check.sh` local-trust flow: the page request opens a session and `/auth/session` then returns the local owner; DNS-rebound and cross-site requests refused (no cookie); `/local/mode` true; after a second account registers, 409 `not_single_person_install`; with local trust off `/local/session` is 404 and `/local/mode` false (the password form stays).
- Live, through the real Next proxy against a fresh database `aidrive_t1` (API on 127.0.0.1:8082 with local trust, web on :3002): `curl` with `Host: evil.test:3002` got `not_local_browser`, a cross-site request got `cross_origin_request`. In the browser pane, a cold open of http://localhost:3002 showed "On this computer as owner@local.invalid" and the spaces panel; no password field, no Create account, no Sign out; creating a space worked. The one console error is the expected 401 of the first session probe.

Not yet: the dev API on 8080 and the web app on 3000 run this only after `main` is updated, the API rebuilt and restarted, and the web dev server restarted; a cold newcomer has not tried it on another machine; the first page load still makes one 401 request before opening the local session.

### Assistant keys live 30 days (pilot blocker 1) — 2026-11-23

Started by another Claude Code session (transcript ed022f6d, stopped about 2026-11-22 23:01 before committing) and completed here at the adviser session's decision; that session's uncommitted files were backed up first and are contained unchanged in this commit, plus the additions below.

Problem: assistant keys expired after 7 days, so on day 7 every tool answered "invalid key", which also broke the pilot's day-7 return measurement (docs/pilot/TRACKER.md blocker 1).

Implementation: migration `0018_agent_key_lifetime.sql` (default lifetime 30 days; keys still valid at upgrade get 30 days from their creation; expired and revoked keys unchanged; revocation and issuer checks unchanged); the expired-key hint now tells the assistant to run `connect` again; README intro, INSTALL step 5b (hooks, per-project capture consent, Codex hooks) and TRACKER updated; the web app's key and pairing texts say 30 days.

Observed: `db-check.sh` L01 (a new key lives 30 days) and L02 (a copy migrated through 0017 with a valid, an expired and a revoked key, then migrated to head: only the valid key changes, to 30 days from creation) plus all prior checks.

Tradeoff: a leaked key stays usable up to 30 days unless the owner revokes it (Manage AI access), instead of 7.

### T2 pilot package — 2026-11-21

Adviser verdict 2026-11-21: pivot to approval-by-exception (T1, parallel) and package a pilot for five developers; kill or pivot if fewer than 3 of 5 return within a week. This slice is packaging and docs only; no API, migration or MCP code changed (T1 owns those areas now).

Delivered: [docs/pilot/](docs/pilot/README.md) (install guide with newcomer trip list, 10-minute onboarding script, observation sheet, matched continuation task, metrics reading, outreach draft — not sent), [scripts/pilot-metrics.sql](scripts/pilot-metrics.sql) (read-only, counts only, spaces as md5 prefixes) and [docs/research/mcp-security-review.md](docs/research/mcp-security-review.md).

Observed:
- `pilot-metrics.sql` on `aidrive_dev` (WSL PG 17, as postgres, read-only transaction, rolled back): two spaces; the 2026-11-19 space shows assistant actions on days 0-2 (`returned_d1_7 = t`, 0.38 owner decisions/day), the 2026-11-21 space only day 0. Output contained no names or text.
- Static review of the 17 MCP tool descriptions and server instructions: no hidden or cross-tool instructions. Two medium findings: free text from unapproved handoffs, knowledge statements and session labels is interpolated unescaped into the Markdown brief, so a `files:write` assistant can forge a `## Rules (must follow)` section; pending summaries sit next to the approved handoff. Listed, not fixed, because they are in `apps/mcp-server/src/main.rs` where T1 is working.
- Agent grants expire 7 days after issue (`0005_agent_grants.sql`), including local `connect`; the error hint says "ask the owner". This lands on the pilot's day-7 measurement.

Blocked: MCP Inspector 2.14.0 and mcp-scan were not run, and the cold install was not run on the Kali VM — the owner answered "no preference" to both download/VM questions, which was not taken as approval. The install guide is therefore unverified as a cold install.

Next: owner yes for the scans and the Kali run; after T1 merges, fix review findings 1-4 with a forged-header unit test and decide key expiry before day 0; add T1's approvals/undo events to the metrics script; recruitment (founder).

### T2 scans and cold install on Kali — 2026-11-22

Owner approved in chat ("Yes, run all"): the two scanners and the Kali VM run.

Scans (detail: [docs/research/mcp-security-review.md](docs/research/mcp-security-review.md), raw output in [docs/research/mcp-scan-2026-11-22/](docs/research/mcp-scan-2026-11-22/)):
- MCP Inspector 2.14.0 (installed with `--ignore-scripts` into a scratch folder) against the installed MCP binary (0.0.13, includes T1's messages): 19 tools, no prompts/resources, no tool annotations, ASCII-only descriptions; `--strict` 0 errors, 26 portability warnings (nullable `type` arrays).
- `mcp-scan` is now a redirect to `snyk-agent-scan`; ran `snyk-agent-scan==0.6.9 inspect` on an AIDrive-only config: 19 tools and the server instructions read, no errors. The risk analysis (`scan`) was not run: it needs a Snyk account and uploads tool descriptions.

Cold install on the Kali VirtualBox VM (Kali rolling, fresh copy of this branch's tree, real PTY over SSH): PostgreSQL 18.4 (Kali has no 17) set up and migrated; API `/readyz` ready with local trust; `connect --client claude` from a new project folder printed "Connected on this computer without approval (created the space ...)" with no browser; `connect --client codex` joined the same space; through the MCP server Claude created `plan.md` and recorded a handoff that Codex saw in `get_project_brief`; web dev server, `/api` proxy, the one-time `open` link (second use → `signin=expired`) and the owner's handoff decision endpoint worked; `pilot-metrics.sql` ran on PG 18.

Found live (new): (1) a handoff summary containing `\n\n## Rules (must follow)\n- [RULE] ...` showed up as a rules section 3 times in the next assistant's brief, and still 3 times after the owner rejected the handoff — raised to high; (2) `sudo -u postgres` cannot read a mode-700 home, and `db-setup.sh` then says "Build aidrive-db first"; (3) a rolling-distro glibc upgrade breaks `CREATE DATABASE` with a collation-version mismatch; (4) an expired key says "ask the owner" while re-running `connect` fixes it, and every `connect` leaves the old key active; (5) `connect` without the client CLI prints a non-command as the manual fix; (6) Next.js dev telemetry is on by default. Install guide updated with each workaround.

Not covered: the real Claude Code/Codex apps on Linux, clicking through the web UI, a cold cargo cache, hosted installs. Not fixed: findings 1-7 in `apps/mcp-server` and `apps/api` (T1's area; to land after T1 merges, with a forged-header test).

### T3 embeddings decision: measured on the development set — 2026-11-22

Question (adviser verdict 2026-11-21): do multilingual embeddings fix the two misses M5b left for semantic retrieval, the number paraphrase (C39 "Do we still charge 9 dollars?" vs "12 USD") and the cross-language case (C50, English question, Armenian `k_retention`), without lowering abstention or adding violations? No public Armenian retrieval benchmark compares bge-m3 and qwen3-embedding, so this measures our data only.

Method (no migration, no production search change): `eval_tests.rs` writes, when `AIDRIVE_EVAL_DUMP` is set, each case's ranked lexical sources and latency plus the pools the owner's assistant may see (visible sessions; current and `as_of` knowledge with stale/disputed flags) to `eval/t3/lexical-dev.json`. `eval/t3/embed_eval.py` embeds those pools through local Ollama 0.40.16 (GPU, RTX 5060 Laptop 8 GB) and scores every variant with the harness's rules. Vector search runs only inside the API-visible pool, as an RLS-scoped pgvector query would; exact cosine over 59 events + 29 items equals an exact pgvector scan, so pgvector (0.8.7 already in WSL, above the 0.8.2 CVE-2026-3172 floor) was not needed to measure. Text embedded: event body; knowledge `topic statement` (the tsvector's text). qwen3-embedding gets its query instruction prefix; bge-m3 none. RRF k=60 over the lexical and vector top-10 per tool. `tau` = highest top-1 similarity of any abstention question, i.e. tuned on these very cases. "Gated" = the vector list joins RRF only when lexical returned something for that tool. Two runs gave identical results.

| Setting | recall@10 | EN | HY | session | knowledge | changed | temporal | flags | abstention (hits) | violations | C39 | C50 | C11 | C51 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| lexical planner (M5b, reproduced) | 0.95 | 0.92 | 0.98 | 0.97 | 1.00 | 0.83 | 1.00 | 3/4 | 5/6 (2) | 0 | 0/1 | 0/1 | 1/2 | 2 hits |
| random-k, sampled (analytic 10/N 0.32) | 0.29 | 0.24 | 0.35 | 0.05 | 0.44 | 0.33 | 1.00 | 1/4 | 0/6 (120) | 0 | 1/1 | 0/1 | 0/2 | 20 |
| qwen3-embedding:0.6b vector | 0.91 | 0.96 | 0.87 | 0.78 | 1.00 | 1.00 | 1.00 | 4/4 | 0/6 (120) | 0 | 1/1 | 1/1 | 2/2 | 20 |
| qwen3 vector, tau 0.530 | 0.70 | 0.84 | 0.56 | 0.53 | 0.88 | 0.83 | 0.75 | 2/4 | 6/6 (0) | 0 | 1/1 | 0/1 | 0/2 | ok |
| qwen3 hybrid RRF | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 4/4 | 0/6 (120) | 0 | 1/1 | 1/1 | 2/2 | 20 |
| qwen3 hybrid RRF, tau 0.530 | 0.97 | 0.96 | 0.98 | 0.97 | 1.00 | 1.00 | 1.00 | 3/4 | 5/6 (2) | 0 | 1/1 | 0/1 | 1/2 | 2 |
| qwen3 hybrid RRF, gated | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 4/4 | 5/6 (20) | 0 | 1/1 | 1/1 | 2/2 | 20 |
| bge-m3 vector | 0.97 | 0.96 | 0.98 | 0.93 | 1.00 | 1.00 | 1.00 | 4/4 | 0/6 (120) | 0 | 1/1 | 1/1 | 2/2 | 20 |
| bge-m3 vector, tau 0.585 | 0.71 | 0.80 | 0.62 | 0.65 | 0.94 | 0.17 | 0.50 | 4/4 | 6/6 (0) | 0 | 0/1 | 1/1 | 2/2 | ok |
| bge-m3 hybrid RRF | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 4/4 | 0/6 (120) | 0 | 1/1 | 1/1 | 2/2 | 20 |
| bge-m3 hybrid RRF, tau 0.585 | 0.98 | 0.96 | 1.00 | 1.00 | 1.00 | 0.83 | 1.00 | 4/4 | 5/6 (2) | 0 | 0/1 | 1/1 | 2/2 | 2 |
| bge-m3 hybrid RRF, gated | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 4/4 | 5/6 (20) | 0 | 1/1 | 1/1 | 2/2 | 20 |

Cost (observed): both models 1024 dimensions, float32 index for this set 368 KiB (≈4 KiB per source); model on disk 639 MB (qwen3) / 1.2 GB (bge-m3), 1.2 GB / 0.7 GB VRAM loaded. Query embedding median 24 ms, p95 28 ms (qwen3) / 23 ms, 26 ms (bge-m3), against lexical search median 7 ms (max 14 ms) end to end. Indexing the 59 events warm: 0.7 s per model; first call after install 12-35 s (model load). Violations are 0 in every row by construction, because vector candidates come only from the API-visible pool; a production index must keep that property (vectors under the same RLS and deletion path as their source).

What it says: (1) Vector alone is not a replacement: no row both answers and abstains; qwen3 alone is weak in Armenian (HY 0.87, session 0.78), bge-m3 alone is close to lexical in Armenian (0.98). (2) Ungated hybrid answers everything but never abstains (0/6). (3) A similarity threshold, even one fitted to these six questions, restores 5/6 abstention but then fixes only one of the two target misses per model (qwen3 keeps C50, bge-m3 keeps C39) — absolute cosine does not separate "unanswerable" from "answerable in another language/wording". (4) Gated hybrid passes the decision rule on dev for both models: C39, C50 and C11 fixed, abstention 5/6 unchanged, 0 violations; the cost is the one unanswerable question lexical already matched (C51, "use") now returns 20 sources instead of 2, and the gate cannot help a question that shares no word with any source (none in this set, so untested).

Decision: not adopted for production yet. Candidate frozen for the held-out run: bge-m3 (Ollama `bge-m3`, digest 790764642607), gated RRF k=60, no threshold, embeddings of event bodies and `topic statement`; qwen3-embedding:0.6b as the second candidate (smaller, weaker in Armenian alone). Adopt only if the held-out set below shows the same pattern; otherwise stay lexical. Tradeoff: a local model dependency (1.2 GB, a running Ollama, an index to keep in step with deletion and visibility) for semantic recall the lexical planner cannot reach. These are development-set numbers: the gate and both tau values were chosen with these cases in view.

#### Held-out set specification (next step, not built)

- History: a second synthetic project, unrelated to TutorBook (e.g. a small Yerevan bakery's ordering and delivery app), written by someone who has not read `eval/dev` cases; never shown to tuning sessions. Same shape: 3+ files with versions, 5+ visible sessions (~80 events), a private session, another workspace, a deleted session, ~35 proposals with 3+ supersessions, pending items, a stale item and a disputed item. English and Armenian, plus mixed-language turns.
- 100 cases, each with `lang`, `category`, `tool`, `expect`, `forbid`, optional `as_of`/`flag`; at least 10 per language and per category (tags may overlap): session recall 20, knowledge recall 16, changed facts 10, temporal 10, flags 10, abstention 14, permission 10, plus 10 tagged semantic: 4 number/unit paraphrases, 4 cross-language (EN↔HY both directions), 2 synonym-only questions sharing no word with any source (tests the gate's blind spot). Abstention includes questions that share generic words with sources (like "use").
- Locking: file `eval/heldout/<name>.json` committed with its SHA-256 recorded here before any run; the model digest, prompt prefix, RRF k, gate rule and K are frozen in the same commit. One run per configuration (lexical; each frozen embedding candidate); a held-out case used for tuning moves to dev and is replaced with an unseen one before a new claim (docs/PRODUCT.md protocol).
- Pass for adopting embeddings: hybrid recall@10 ≥ 0.95 overall and ≥ 0.90 per language/category, semantic-tagged recall above lexical, abstention not below lexical's count and ≥ 95% target tracked separately, abstention hits not more than doubled, 0 violations, query p95 ≤ 100 ms on this machine.

Reproduce: `AIDRIVE_EVAL_DUMP=$PWD/eval/t3/lexical-dev.json scripts/api-check.sh` (WSL), then `python -I eval/t3/embed_eval.py` with `ollama pull qwen3-embedding:0.6b` and `ollama pull bge-m3` done; output in `eval/t3/results-dev.json`.

### T3 on the locked held-out set — 2026-11-22

Set: `eval/heldout/bakery.json`, a fictional Yerevan bakery ("Nush", 2 Sep-5 Nov 2026): 6 visible sessions (113 events), an editor-private session, another workspace, a deleted session, 4 files with versions, 49 proposals (8 supersession chains, 6 pending, 6 stale and several disputed items). 100 cases: session 20, knowledge 16, changed facts 10, temporal 10, flags 10, abstention 14, permission 10, semantic 10; EN 55, HY 45. It was written by a fresh subagent told not to open `eval/dev`, `eval/t3`, ROADMAP, docs or source code. Locked at e0f4220 (sha256 da1d973d…). One seeding fix came before any result: a pending proposal cited the editor's private session, which the proposing assistant cannot cite. Re-locked at 4c6e16e (sha256 898b0766…, `frozen.json` records the amendment). The configuration was frozen in `eval/heldout/frozen.json` before the run: model digests, dev tau, RRF k=60, K=10, the qwen3 prompt. `embed_eval.py --frozen` refuses a mismatch. One run per configuration, on main 80f87e0 plus this branch.

| Setting | recall@10 | EN | HY | session | knowledge | changed | temporal | semantic | flags | abstention (hits) | violations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| lexical planner (M5b) | 0.65 | 0.91 | 0.33 | 0.72 | 0.81 | 0.70 | 0.60 | 0.40 | 5/10 | 5/14 (94) | 0 |
| random-k, sampled (analytic 0.27) | 0.20 | 0.25 | 0.14 | 0.03 | 0.12 | 0.10 | 0.60 | 0.05 | 5/10 | 0/14 (280) | 0 |
| qwen3 vector | 0.89 | 1.00 | 0.76 | 0.90 | 0.94 | 1.00 | 0.90 | 0.85 | 7/10 | 0/14 (280) | 0 |
| qwen3 vector, dev tau 0.530 | 0.62 | 0.84 | 0.34 | 0.60 | 0.75 | 0.80 | 0.60 | 0.30 | 6/10 | 9/14 (8) | 0 |
| qwen3 hybrid RRF | 0.89 | 1.00 | 0.76 | 0.90 | 0.94 | 1.00 | 0.90 | 0.85 | 7/10 | 0/14 (280) | 0 |
| qwen3 hybrid, dev tau | 0.72 | 0.93 | 0.44 | 0.78 | 0.88 | 0.80 | 0.60 | 0.50 | 6/10 | 5/14 (95) | 0 |
| qwen3 hybrid, gated | 0.78 | 0.98 | 0.53 | 0.85 | 0.88 | 0.90 | 0.60 | 0.65 | 7/10 | 5/14 (160) | 0 |
| bge-m3 vector | **0.98** | 0.99 | **0.97** | 0.97 | 1.00 | 1.00 | 1.00 | 0.90 | **10/10** | 0/14 (280) | 0 |
| bge-m3 vector, dev tau 0.585 | 0.70 | 0.83 | 0.54 | 0.55 | 0.88 | 0.80 | 0.90 | 0.35 | 7/10 | **13/14 (3)** | 0 |
| bge-m3 hybrid RRF | 0.97 | 0.98 | 0.97 | 0.97 | 1.00 | 1.00 | 1.00 | 0.85 | 10/10 | 0/14 (280) | 0 |
| bge-m3 hybrid, dev tau | 0.81 | 0.92 | 0.67 | 0.78 | 0.94 | 0.90 | 0.90 | 0.55 | 7/10 | 5/14 (94) | 0 |
| **bge-m3 hybrid, gated (frozen primary)** | 0.84 | 0.95 | 0.69 | 0.93 | 0.94 | 0.90 | 0.60 | 0.65 | 9/10 | 5/14 (160) | 0 |

Cost here (113 events + 49 items): float32 index 648 KiB. Query embedding median 26-27 ms, p95 37 ms (qwen3) / 75 ms (bge-m3). Lexical search median 12 ms, max 44 ms.

Result against the pass rule written before the run: **the frozen primary fails** (recall 0.84 < 0.95, HY 0.69 < 0.90, temporal 0.60, abstention hits 160 > 2×94). Embeddings are **not adopted**.

What the held-out set shows (observed):
1. **The M5b lexical planner does not generalise.** It scores 0.65 here against 0.95 on dev, Armenian 0.33 against 0.98, and many Armenian questions return nothing at all. Its abstention is 5/14, and temporal recall (0.60) is no better than random-k (0.60, because the `as_of` pools are small). The dev numbers were tuned to the dev history.
2. **bge-m3 retrieves almost everything.** Vector alone reaches 0.98, Armenian 0.97, semantic 0.90 and flags 10/10. It never returns a forbidden source, because candidates come only from the API-visible pool. qwen3-embedding:0.6b is clearly weaker in Armenian (0.76).
3. **No retrieval-only rule both answers and abstains.** The dev-fitted threshold gives bge-m3 13/14 abstention but drops recall to 0.70. The gate keeps lexical's abstention count but also keeps its Armenian blind spot, because the vector side never runs when lexical finds nothing. On this data, absolute cosine does not separate unanswerable questions from answerable ones.
4. **Lexical errors dominate.** Wherever lexical ranks first (gated, thresholded), recall drops below vector alone.

Hypothesis, not yet measured: use bge-m3 vector retrieval (or ungated RRF) for recall, and move abstention out of zero-hit retrieval into an evidence-sufficiency step. A reranker or the answering model would judge whether the returned sources answer the question; docs/PRODUCT.md already defines abstention at the answer level. This set's failures are now known, so testing that hypothesis needs a fresh held-out project (protocol: move used cases to dev and replenish). Tradeoff of that direction: a 1.2 GB local model and Ollama on every install, a vector index under the same RLS and deletion path as its sources, and a second step that costs latency or model calls.

Reproduce: `AIDRIVE_EVAL_SET=$PWD/eval/heldout/bakery.json AIDRIVE_EVAL_DUMP=$PWD/eval/heldout/lexical-heldout.json scripts/api-check.sh` (WSL), then `python -I eval/t3/embed_eval.py --set eval/heldout/bakery.json --dump eval/heldout/lexical-heldout.json --out eval/heldout/results-heldout.json --frozen eval/heldout/frozen.json`.

### Armenian lexical fix, dev then one held-out run — 2026-11-23

Two hypotheses for the held-out Armenian failure, designed only from examples written for this change, without reading held-out cases or `lexical-heldout.json` (`apps/api/src/search.rs`):
1. The Armenian marks ՛ ՜ ՞ (U+055B/055C/055E) sit inside a word ("Որքա՞ն") and split it into two non-words. They are now removed before splitting.
2. Question forms were longer than stored forms. Changes:
   - Verb endings were added (-եցինք/-եցին/-եցի/-եց/-ել/-ելու/-ելով/-ած/-ենք/-եք/-են) along with -ները/-ներն.
   - The longest matching ending is removed, and a 3-letter stem is allowed after the article or after a 3+ letter ending.
   - An Armenian stem of 7+ letters also matches a truncated prefix of n-3 letters (floor 6), as one OR group per term, e.g. `('մատակարար':*|'մատակա':*)`.

Two dev-only corrections came before the held-out run:
- The forms of "to have" (ունեմ…ունեին) became stopwords, as have/has/had already are in English. Stripped, they turned into `'ուն':*`.
- The truncation floor was raised from 5 to 6. On dev, the 5-letter prefix "աշխատ" matched "աշխատում" (works) for a question about "աշխատակից" (employees), which broke abstention.

| Set | recall@10 | EN | HY | session | knowledge | changed | temporal | semantic | flags | abstention (hits) | violations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| dev, M5b before | 0.95 | 0.92 | 0.98 | | | | | | 3/4 | 5/6 (2) | 0 |
| dev, this change | 0.96 | 0.92 | 1.00 | 1.00 | 1.00 | 0.83 | 1.00 | | 3/4 | 5/6 (2) | 0 |
| held-out, M5b (T3 baseline) | 0.65 | 0.91 | 0.33 | 0.72 | 0.81 | 0.70 | 0.60 | 0.40 | 5/10 | 5/14 (94) | 0 |
| held-out, this change (one run) | 0.65 | 0.91 | 0.33 | 0.72 | 0.81 | 0.70 | 0.60 | 0.40 | 5/10 | 4/14 (96) | 0 |

Result: **no held-out gain.** Recall and every category are unchanged, and abstention is one case worse. Neither hypothesis was the cause of the held-out Armenian failures: the marks and the inflection gaps either do not occur there, or fixing them does not change the top 10. The change is kept, since it fixes real word splitting and adds a dev case to the stem tests, but it is not a fix for the 0.33.

Not measured, for whoever picks this up: what the held-out Armenian misses actually look like. Candidate causes include Armenian questions about sources stored in English (lexical cannot cross languages), synonyms, or transliterated names. Any of these points back to the T3 hypothesis (vector retrieval for recall). Looking would use this held-out set for tuning, so under the protocol its cases move to dev and a fresh set is needed first.

### T4 local search with an evidence judge: development measurement — 2026-11-23

Question: does the T3 hypothesis hold, that bge-m3 retrieves and a separate evidence-sufficiency step abstains? Everything runs locally (Ollama, RTX 5060 Laptop 8 GB); no session text leaves the machine. No production change yet.

Method (`eval/t4/judge_eval.py`, reusing the T3 harness dumps and scoring): retrieval is lexical + bge-m3 (digest 790764642607), ungated RRF k=60, top 10 per tool, candidates only from the API-visible pools. The judge is qwen3.5:9b (Ollama, thinking off, temperature 0, seed 7, JSON schema output) given the question and the numbered candidates (first 500 characters each). J1: if the judge says the sources do not answer, return nothing; otherwise return the retrieval result unchanged. J2: also keep only the sources the judge marks relevant. One prompt change was made, from the four tutorbook misses of the first prompt (terse keyword and statement-to-check questions, mostly Armenian): "a source that states, confirms, contradicts or updates the asked fact answers it ... When unsure, answer true". Bakery cases were not read. Bakery is no longer held-out: it was run by T3 and by the Armenian fix, so it now counts as development data.

| Set, setting | recall@10 | EN | HY | session | knowledge | changed | temporal | semantic | flags | abstention (hits) | violations |
|---|---|---|---|---|---|---|---|---|---|---|---|
| tutorbook, lexical | 0.95 | 0.92 | 0.98 | 0.97 | 1.00 | 0.83 | 1.00 | | 3/4 | 5/6 (2) | 0 |
| tutorbook, hybrid ungated | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | 1.00 | | 4/4 | 0/6 (120) | 0 |
| tutorbook, **J1** | 0.98 | 1.00 | 0.96 | 1.00 | 1.00 | 0.83 | 1.00 | | 4/4 | **6/6 (0)** | 0 |
| tutorbook, J2 | 0.93 | 0.96 | 0.90 | 0.93 | 1.00 | 0.83 | 1.00 | | 4/4 | 6/6 (0) | 0 |
| bakery, lexical | 0.65 | 0.91 | 0.33 | 0.72 | 0.81 | 0.70 | 0.60 | 0.40 | 5/10 | 5/14 (94) | 0 |
| bakery, hybrid ungated | 0.97 | 0.98 | 0.97 | 0.97 | 1.00 | 1.00 | 1.00 | 0.85 | 10/10 | 0/14 (280) | 0 |
| bakery, **J1** | **0.95** | 0.93 | **0.97** | 0.93 | 1.00 | 1.00 | 1.00 | 0.85 | 10/10 | **14/14 (0)** | 0 |
| bakery, J2 | 0.91 | 0.89 | 0.94 | 0.85 | 1.00 | 1.00 | 1.00 | 0.70 | 10/10 | 14/14 (0) | 0 |

Latency (observed): query embedding median 23 ms, p95 27-35 ms; judge median 0.63-0.74 s, p95 1.0-1.6 s. Single judge calls took 12 s and 23 s; the two models together (1.2 + 6.6 GB) nearly fill 8 GB of VRAM, so Ollama sometimes reloads one of them. Lexical alone is 7-12 ms.

What it says: (1) The split works on both sets. The judge abstains on every unanswerable question (20/20) while recall stays at 0.95-0.98, against lexical's 10/20 abstention and 0.65 bakery recall. (2) Filtering with the judge (J2) costs recall and buys nothing for the metrics measured here, so J1 is the candidate. (3) Remaining J1 misses are false abstentions on answerable questions (tutorbook C41 "Բետայի ամսաթիվը"; bakery 2 false abstentions plus 4 partial recalls). (4) The cost is about 0.7 s per search and a 6.6 GB model, against 10 ms today.

Frozen candidate for a fresh held-out set: J1 with exactly this embedder, judge, prompt (`SYSTEM` in `eval/t4/judge_eval.py` as committed), K=10, RRF k=60, 500-character snippets, fail-open on unparsable output. Pass rule, written before that run: recall@10 ≥ 0.90 overall and ≥ 0.85 per language and category; abstention ≥ 90% of unanswerable questions; violations 0; judge p95 ≤ 2 s on this machine. If it fails, search stays lexical in production. Not done: the fresh held-out set (a different project history by an independent author, as T3 did), the production path (API calls to Ollama, a vector index under the same RLS and deletion path as its sources, a lexical fallback when Ollama is not running), and answer-level evaluation.

Reproduce: `python -I eval/t4/judge_eval.py --set eval/dev/tutorbook.json --dump eval/t3/lexical-dev.json --out eval/t4/results-tutorbook.json` and the same with `--set eval/heldout/bakery.json --dump eval/heldout/lexical-heldout.json --out eval/t4/results-bakery.json`, with `ollama pull bge-m3` and `ollama pull qwen3.5:9b` done.

### File index with keep-until checks; rules every assistant gets on connect — 2026-11-24

Owner request (2026-11-23): an assistant that writes a file somewhere registers it with where it lives (machine, worktree, place) and how long it is needed, e.g. 3 days; after that AIDrive checks whether it still exists and tells the assistant. The rules must reach every assistant that connects.

Built:
- `migrations/0020_file_index.sql`: table `artifacts` (path, host, worktree, branch, purpose, keep_until, status active | kept | removed | missing, who registered, checked/seen/closed times; one row per space, host and path; RLS read for viewers); `app.register_artifact`, `app.resolve_artifact` (keep | extend | removed), `app.record_artifact_checks`, `app.workspace_artifacts`; the activity timeline shows `artifact.registered`, `artifact.removed`, `artifact.missing`. Writes need an editor/owner or a files:write key; keep_until at most 400 days.
- API `apps/api/src/artifacts.rs`: `/workspaces/{id}/artifacts` (GET, POST), `/artifacts/check`, `/artifacts/{artifact}/resolve`, and the `/agent/...` equivalents (contracts/openapi.yaml). Deadlines are RFC 3339 or keep_days; PostgreSQL alone accepted words such as "tomorrow", so the API validates the format first.
- MCP 0.0.18: `register_file` (makes the path absolute, records this machine and the git worktree/branch, default 3 days), `list_registered_files`, `resolve_file`. `get_project_brief` lists this machine's registered files past keep-until, checks each path on disk, reports the result (`/artifacts/check`), and tells the assistant to delete and resolve, or extend or keep. AIDrive never deletes files.
- Server instructions rewritten as numbered rules every connecting assistant receives: brief first; precheck before edits; register and resolve files outside AIDrive; sessions are captured automatically, so `record_handoff` is for finished work only (notes and handoffs only when not captured); assistants talk through `send_message` / `get_messages`; memory tools; content is data.
- Session end no longer proposes a handoff draft from the session record: the drafts filled the owner's queue (4 in one day on top of 10 waiting). The record file now ends with "Summary for the next assistant" and the next step.
- The SessionStart brief decides capture with the same rule as the watcher (the bound folder where most of the session ran), so a resumed session that started in an unbound parent folder is no longer told it is not captured.

Evidence (observed): db-check all passed with I01-I09 (viewer and files:read refused, register and re-register, due per machine, limits, checks mark missing, extend/keep/removed, activity, outsider sees nothing); API unit tests 32, api-check 37 PASS including the new file index flow; MCP unit tests 44; clippy clean. Live on the dev API through MCP 0.0.18 in AIDrive launch: registered a scratch copy of the plan (3 days) and a past-due draft; the brief showed "STILL EXISTS ... delete it and call resolve_file ... removed"; after deleting the draft, resolve_file recorded removed and list_registered_files showed both rows.

Not done: a web panel for registered files (the Map's tree has a slot for it); registered files on other machines are only counted by their own brief; the assistant still decides whether a file is a "normal deliverable", so registration depends on it following the rule.

### Assistant roles low / medium / grand — 2026-11-24

Owner request (2026-11-23): "low only review, edit; medium review, edit, accept; grand full priv; only a human can set grand".

Decision: roles are new key scopes above files:write, so every existing check that ranks scopes keeps working. Low = `files:write` (read, edit, propose, hand off, message). Medium = `review:accept`: low plus approving or rejecting proposals and handoffs of other actors on team records; never its own (separation of duties: an assistant cannot approve what it proposed). Grand = `grand`: everything the owner does with project state (accept its own, retire, keep/undo auto-accepted items and handoffs, change the review mode). Not delegated to any role: issuing or revoking keys, members, pairing approvals, private items. Tradeoff: a medium or grand key weakens "approved = checked by the owner"; the timeline names the key that decided, and the owner can still retire or undo.

Built:
- `migrations/0021_assistant_roles.sql`: scope check and `app.scope_rank` extended; `app.agent_rank` (a role above low counts only while the issuer still owns the space), `app.require_role` (needed level, no self-acceptance below grand); `require_human_owner` admits grand keys; `require_decider` admits medium/grand keys for team records only; `decide_item`, `retire_item`, `confirm_item`, `undo_item`, `decide_handoff` call `require_role` instead of refusing every assistant; `issue_agent_grant` refuses assistant context; `current_writing_agent` accepts any role from low up. Columns `decided_agent` (who approved or rejected) and `acted_agent` (who kept, undid or retired) on knowledge items and handoffs, stamped by triggers except for automatic acceptance; the activity timeline shows them. Function bodies were regenerated from 0015/0017/0020 with targeted substitutions.
- API: `SCOPES` adds `review:accept` and `grand`; local trust keeps its own list (`LOCAL_SCOPES`, at most files:write), because it connects without anyone approving. Decision, retire, keep, undo and review-mode handlers accept an assistant key and are also served under `/agent/...`; the database decides what the role may do (403 otherwise).
- MCP 0.0.19: `list_pending`, `decide_proposal`, `decide_handoff`; rule (7) in the server instructions explains the roles.
- Web: Manage AI access and the pairing page offer Low / Medium / Grand; the key list shows the role.

Evidence (observed): db-check all passed including Q01-Q07 (only the owner issues roles, never an editor or an assistant; low cannot approve; medium approves others' proposals and handoffs, recorded as its decision, never its own, cannot retire or change the mode; grand accepts its own, rejects, retires, decides handoffs, changes the mode; no assistant decides private items; a medium key acts as low after its issuer loses ownership; medium writes files). API unit tests 32; api-check 38 PASS including the roles flow over HTTP (also: the activity timeline names medium and grand on approvals, grand on the retire); earlier tests that expected no agent decision route now expect 403 for low keys. MCP unit 45, clippy clean, web tsc and lint clean. Dev DB migrated to 0021, API restarted, MCP 0.0.19 installed; the Manage AI access list showed the three roles live. No key was issued in the live check.

Not done: a role change for an existing key (issue a new key and revoke the old one); a per-role view in the web review queue.

### T2: embedded PostgreSQL and release 0.1 groundwork — 2026-11-24

Owner approvals in chat: the `postgresql_embedded` crate and PostgreSQL binary download, a read-only crates.io name lookup, and the Kali run after Windows. License chosen by the owner: Apache-2.0 OR MIT. Nothing pushed, tagged or published; no tokens used.

Decision: `aidrive-api --embedded-db` (default feature `embedded-db`) using `postgresql_embedded` 0.20.2 with rustls and the PostgreSQL archive bundled at build time (17.12.0, pinned in `.cargo/config.toml`). Tradeoff vs. requiring a system PostgreSQL: one command and no WSL/Docker, at the cost of a 49 MB archive inside the binary and a GitHub download during the build. 0.20.2 is the last 0.20.x on sqlx 0.8 (0.20.3+ moved to sqlx 0.9); `postgresql_archive`/`commands` are pinned to 0.20.2 because 0.20.3+ need Rust 1.94 (found on Kali). MSRV is now 1.92.

Implementation: `apps/api/src/embedded.rs` — data under `%LOCALAPPDATA%\AIDrive\postgres` or `~/.local/share/AIDrive/postgres` (`AIDRIVE_DATA_DIR`), `127.0.0.1:5544` (`AIDRIVE_EMBEDDED_PG_PORT`), generated superuser and app passwords kept in the data folder (0600 on Unix), the roles of `db/bootstrap.sql`, a database with the builtin `C.UTF-8` provider (Armenian `lower()` without host locales), compiled-in migrations applied on every start as `aidrive_owner`, reuse of a server a killed API left running, stop on Ctrl+C. The `DATABASE_URL` path is unchanged.

Release groundwork: LICENSE-APACHE, LICENSE-MIT, crate metadata, `deny.toml`, public README (install, limits, demo GIF), SECURITY.md, CONTRIBUTING.md, `scripts/demo.sh` + `scripts/mcp-call.py`, `docs/demo/demo.gif` (VHS, hidden prelude, frames checked by eye: no paths or names), unrun `.github/workflows/release.yml`, [docs/RELEASE.md](docs/RELEASE.md). `url` 2.5.9 (RUSTSEC-2026-0592).

Observed:
- Windows 11 native: fresh data folder → `/readyz` ready; `connect --client claude` with local trust created the space without approval; after a hard kill the next start reused the running server; data, the `builtin C.UTF-8` database and migrations through the latest version survived restarts.
- Kali VM, cold (Rust 1.92 via `rustup override`, no `~/.theseus`): `cargo install --locked --path apps/api` 3:15, `apps/mcp-server` 1:48; `aidrive-api --embedded-db` ready; password files 600; `connect` without approval; `scripts/demo.sh` with the installed binaries showed the auto-accepted decision, revision 2 from Codex, a 409 for a write from the old version, and the brief warning "evidence changed since approval"; SIGINT stopped API and PostgreSQL (ports free). With Rust 1.88 the install fails with a clear "requires rustc 1.92".
- After rebasing onto `c4bc3c8`: fmt and Clippy clean (with and without default features), unit tests API 31 / MCP 42, `cargo deny check` advisories/bans/licenses/sources ok, `db-check.sh` all passed, `api-check.sh` 36 PASS.
- `cargo publish --dry-run`: `aidrive-mcp-server` passes; `aidrive-api` fails verification because `sqlx::migrate!` points outside the crate; `aidrive-db` passes but is only useful inside the repository. Crate names free. Packaged files contain no secrets, `.env*` or local paths.
- MCP: finding 1 was fixed on main by T1; this branch also flattens `search_sessions` labels and snippets (unit test).

Not yet: the migrations move needed for a crates.io `aidrive-api`; the repository URL (`OWNER` placeholder); version bump to 0.1.0; PostgreSQL License notice in release archives; the release workflow has never run; Claude Code/Codex apps on Linux; `connect` panics on a closed stdout pipe.

### T2: fixes from T1's review of the release branch — 2026-11-24

Review: `t1-review-of-t2-release-2026-11-24.md` (shared notes). Fixed here:
- H, release workflow: default `contents: read`; the build jobs get only a read-only token on the Build step (PostgreSQL download rate limit); `contents: write` only in the `release` job, which runs no build; no cache in release builds; THIRD-PARTY-NOTICES.md (PostgreSQL and theseus notices) shipped in every archive. CI `msrv` job moved to 1.92, and CI builds get the read-only token for the same download.
- M, embedded PostgreSQL: a running server is adopted only if it reports our own data folder (`data_directory`), and on Unix every connection goes through the 0700 socket folder; `pg_hba.conf` is rewritten from `password` to `scram-sha-256` (existing installs too, with a reload when adopting); the data folder is 0700 and password files are forced to 0600 on every start; SIGTERM stops the API and PostgreSQL like Ctrl+C; `--embedded-db` refuses a non-loopback `AIDRIVE_API_ADDR`.

Observed: Windows (existing data folder with `password` lines → rewritten to `scram-sha-256`, ready; reuse after a hard kill; a second data folder on the same port refused instead of adopted; `0.0.0.0` refused). Kali (rebased tree, Rust 1.92): a 0644 `app.pw` became 0600, folder 0700, scram in `pg_hba.conf`, API on the private socket; after `kill -9` the next start reused the server; SIGTERM left no API and no listener on 5544. fmt, Clippy, unit tests (API 32, MCP 42), `cargo deny` ok; `db-check.sh` all passed, `api-check.sh` 36 PASS.

Follow-up after T1's re-check (same day): on Windows there is no private socket, so before any connection the API reads `postmaster.pid` in its own data folder and contacts the port only if that pid is the process listening on `127.0.0.1:<port>` (`netstat -ano`); otherwise it starts its own server, which fails clearly if the port is taken. Live: reuse still works for our own postmaster; with a forged pid in the lock file the API did not connect and started its own server. sqlx gives no way to refuse a server that asks for a clear-text password, so this ordering is the protection. Rebased onto `e2e76db` (0020 file index, 0021 roles): embedded smoke migrates a fresh folder to 21 and `connect` works; fmt, Clippy (Windows and Linux cfg), unit tests API 34 / MCP 46, `cargo deny` ok, `db-check.sh` all passed, `api-check.sh` 38 PASS.

Still open from the review: actions pinned by tag, not SHA (needs a GitHub lookup); per-target SHA-256 pin of the PostgreSQL archive (authenticity); build provenance; the web app is not in the release archives; Windows ACL of the data folder untested; the closed-console case on Windows.

### Disk cleanup on the file index — 2026-11-26

Proposed by Codex, asked for by the owner: the disk was down to 2.1 GB free, 81 GB of it Rust build output in worktrees. Mechanism: index temporary files, build folders, packages and large outputs with path, size, purpose, creating session and a review date; when the date passes or the disk runs low, check whether each is still in use and ask "keep (until when) or remove", showing what it frees; after a decision, update the index. A passed date is a reason to review, never permission to delete.

Built:
- `migrations/0026_artifact_sizes.sql`: registered files get `kind` (file, temp, build, package, artifact), `size_bytes`, `session_ref`, `size_checked_at`; `register_artifact` gains kind, size, session and `refresh` (an automatic re-index of a known open path updates only its size, place and last-seen time, so review dates, notes and decisions stay; a removed path that comes back is registered anew); checks can report measured sizes and also close kept files found gone; the list is ordered by size within status.
- API: `/artifacts` accepts kind, size_bytes, session, refresh and returns them; checks accept size_bytes.
- MCP 0.0.22: `register_file` measures the size and detects build/package folders; the watcher indexes build and package folders (Rust `target` with a Cargo crate, `node_modules` with package.json, `.next`) in folders bound to a space with capture on, once an hour, with a 7-day review date; the brief adds "Disk space on this computer" when free space is under 10% or 20 GB: the biggest registered items, in-use hints (recent writes, uncommitted changes, whether the branch is in main, worktree gone), what removing them would free, and the question. Rule (3) of the server instructions says the same.

Evidence (observed): db-check all passed including D01-D05; API unit 34, api-check 40 PASS (file index flow covers build folders, size-only refresh, bad kind/size); MCP unit 67 (low-disk threshold, build-folder discovery and size, the section text, free space on this disk), clippy clean.

Not done: a web view of sizes; indexing folders outside bound folders; per-file large-artifact detection beyond build and package folders.

### Workspace motion — 2026-11-26

Owner request: beautiful transitions, connections that follow the mouse, a light animated background, and a left-to-right transition when the selected assistant changes. Built in apps/web (workspace console, scene, connector): two soft green light fields drift behind the scene (CSS transform animations, 28 s and 34 s); a light spot and the connector's curve follow a fine pointer through one damped requestAnimationFrame loop that sleeps when the pointer rests; a short light travels along the connector from the assistant to the panel; switching assistant redraws the connector from the assistant toward the panel, wipes the panel's content in from left to right (clip-path, 340 ms, strong ease-in-out) and passes a light over the newly selected button; the assistant buttons enter with a 70 ms stagger. The background picture stays still so the buttons stay on their platforms. Reduced motion keeps only fades; the pointer effects need a fine pointer; hover lift is gated to real pointers; at 760 px and below the spot is off and the light fields dimmer.

Evidence (observed in the browser against the real dev API): the curve bends toward the pointer and the spot follows; on switching, the sweep (620 ms), connector draw (420 ms) and panel wipe (340 ms) run; worst frame 7 ms over one second while they ran; at 360 px document width stays 360 and the spot is hidden; web tsc and lint clean, 14 node tests pass, production build OK.

### Publication block 5: release archives from one commit, cold install on Windows and Kali — 2026-11-26

Commit `4be9d44` (branch `claude/release-block5` = main `8136e55` + the packaging fix: reinstall web deps when the lockfile changes, honour `CARGO_TARGET_DIR`). Both archives built by `scripts/package-release.sh` from a clean tree at that commit (Kali: `git archive 4be9d44`):

| Archive | SHA-256 | Size | Build |
|---|---|---|---|
| `aidrive-windows-x86_64.zip` | `0f829d87834ac2831f0bdefee1b33b58308a445a2e557a40dc40bd181e65dc78` | 58.7 MB | Windows 11, warm cargo cache, 1:42 |
| `aidrive-linux-x86_64.tar.gz` | `eef2ba58bfb2a205f1263fd1b045d4933fb2b3235372c2577b6fbe7f48f68613` | 49.8 MB | Kali VM, Rust 1.92, 3:16 |

Observed, Windows 11 (clean profile simulated in the same Windows account: fresh `USERPROFILE`/`APPDATA`/`LOCALAPPDATA`/`CODEX_HOME`, ports 7590/7591; the owner's 7480, the dev 8080/3000 and the real Claude/Codex config were untouched, checked by hash). Real `claude` and `codex` CLIs registered the sandbox MCP server.
- Cold install (`install.cmd` from the release folder, capture answered in advance): **11.1 s** to running, both clients connected to space `acme-app`; `/` 200 from the API (static web), `/api/readyz` ready; logon script written and pointing at the sandbox binary.
- `down` 1.3 s; start (`aidrive up`, already set up) 2.2 s; `down` 1.0 s.
- Update: previous build (2026-11-24, `569e1cc0…`) installed in 14.8 s, a file written through the API, then `install.cmd` of this release over it: **5.1 s**, `aidrive.exe` replaced, the file still there.
- `uninstall` 2.0 s: hooks removed (`"hooks": {}`), `claude mcp` entry gone, logon script removed, status "stopped".

Observed, Kali VM (fresh `HOME`, stub `claude`/`codex` scripts logging their calls because no client is installed there; real PTY over `ssh -tt`):
- Cold install from the release folder with the capture question answered on the terminal: the prompt appeared after the API was ready (first start 1 s), the answer `y` set consent on for both clients. A second cold install on disk (not tmpfs), answer in advance: **2.6 s**.
- `down` 0.3 s; start 1.0 s; update from the previous build (`9ce250ae…`, installed in 2.7 s) **1.8 s** with the file kept; `uninstall` 0.3 s; the stubs saw `mcp add` and then `mcp remove` for both clients.
- Watcher: started by the first install (`aidrive-mcp-server watch`, port 47821).

Findings (not fixed here):
1. The watcher is one per computer on 127.0.0.1:47821, not one per user or install: on Kali the second install's `uninstall` stopped the first install's watcher, and on Windows the sandbox `uninstall` stopped the owner's real watcher (restarted by its own logon script within seconds; the in-script restart from a non-interactive WMI session did not come up). Any local process can send `stop`. Fix belongs in `watch.rs`: bind per user (port from the profile folder, or a named pipe / Unix socket in the profile folder) and authenticate `stop`.
2. Linux has no logon start for the watcher: `install-hooks` prints "Add … to your login items". After a reboot capture falls back to the hooks until `aidrive up` runs. A systemd user unit or an XDG autostart entry would close it.
3. Windows: the background API inherits every inheritable handle of whoever ran `aidrive up` (Rust std spawns with `bInheritHandles=TRUE`), so a caller that captures the installer's output through a pipe (CI, a wrapper script, PowerShell `| Out-String`) never sees end-of-file. Interactive use (a console, `install.cmd`, `irm | iex`) is not affected. A full fix needs `CreateProcessW` with `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` or `bInheritHandles=FALSE` plus the API opening its own log.
4. Kali `/tmp` is tmpfs, so the 1 s first start there is faster than a disk; the on-disk run is the figure to quote.

Not covered: a real separate Windows user account (needs admin), a reboot (logon start of the watcher), the Claude Code / Codex apps on Kali, macOS, a hosted download (the release URL is still the `OWNER` placeholder; installs ran from the release folder).

### Finding 3 fixed: a piped caller of `aidrive up` no longer hangs on Windows; archives rebuilt — 2026-11-26

Commit `c7937d0` (branch `claude/windows-detach` from main `81c8bd9`, MCP 0.0.23 per-user watcher included; carries block 5's packaging fix and evidence). Fix in three layers, each found by the piped cold install still hanging:
1. `apps/api/src/detach.rs`: the background API starts through `CreateProcessW` with `bInheritHandles=FALSE`, a hidden console of its own (`CREATE_NO_WINDOW`, shared by cmd/pg_ctl/postgres so no window flashes) and null std handles; the API writes its own log (`AIDRIVE_LOG_FILE`) and logs a fatal start error there. Unix: null stdio, own process group (CLOEXEC already keeps other descriptors out).
2. `install-hooks` starts the watcher with std `Command`, which inherits every inheritable handle; PowerShell hands all of its inheritable handles to native programs, so the caller's pipe reached the watcher even with stdout redirected. On Windows the `aidrive-mcp-server` helpers now run through `cmd /c "... > file 2>&1"` created with no inherited handles (`detach::run_to_file`), and `aidrive up` prints the file.
3. One output file per helper call: the watcher keeps the first file open, so a second helper writing the same name never ran (`connect` failed while `install.cmd` exited 0). `install.ps1` now throws when `aidrive up` fails.

Tests: `a_piped_caller_is_not_held_open` (a caller holding an inheritable pipe gets end-of-file while the background process runs; with `std::process::Command` it fails after 10 s), `command_line_quotes_like_argv`, `run_to_file_captures_output_and_exit_code` (path with spaces). API unit 39, MCP unit 74, clippy and fmt clean.

| Archive (from `c7937d0`) | SHA-256 | Size |
|---|---|---|
| `aidrive-windows-x86_64.zip` | `d41f4c8a4d901bf9d90ad9c9064355fb335bf153a3e337570eb485590d397495` | 58.7 MB, Windows 11, 1:23 warm |
| `aidrive-linux-x86_64.tar.gz` | `5a06a3ff0e210bcd6803d62ef9e7ba60ce59c2becd788290790a2a9743373404` | 49.8 MB, Kali, Rust 1.92, 1:26 warm |

Observed, Windows (same sandbox profile method and ports 7590/7591 as block 5; every step of the new release run with its output captured through a PowerShell pipe, the pattern that hung): whole run returned in 41 s. Cold install 13.9 s (both clients connected, `/` 200, `/api/readyz` ready, logon script written); down 1.3 s; start 2.3 s; down 1.2 s; previous release `4be9d44` installed without a pipe (it still has the bug) 12.0 s; update to `c7937d0` 5.7 s with the file kept and the binary replaced; uninstall 2.5 s (hooks, MCP entry, logon script removed). The owner's watcher (per-user 0.0.23, PID 38112) kept running throughout; real Claude/Codex config unchanged by hash.

Observed, Kali (fresh HOME, real PTY, stub clients): capture question answered on the terminal; cold install piped through `cat` 2.7 s; down 0.3 s; start 1.0 s; update from `4be9d44` 1.9 s with the file kept; uninstall 0.3 s. Per-user watcher: the second install got its own watcher and its uninstall stopped only that one; XDG autostart entry written (`~/.config/autostart/aidrive-watch.desktop`).

Findings, not fixed here:
1. `watch.rs` `ensure_running` still spawns the watcher with std `Command`; any caller of `install-hooks` or of `brief --auto` (Claude Code SessionStart hook) hands its inheritable handles to the long-lived watcher. On Windows the hook's output pipe can stay open; whether Claude Code then waits is not observed. The same `detach` approach fits there.
2. An update does not refresh hooks or the logon start: `aidrive up` runs `install-hooks` only for clients not yet in `aidrive.json`, so a Linux install updated from a pre-0.0.23 build keeps "Add … to your login items" and gets no autostart entry. Re-running `install-hooks` when the bundled MCP version changes would close it.
3. `helper-*.out` stays in the logs folder while the watcher that holds it runs; the next `aidrive up` deletes it.
4. Seen in the owner's `watch.log` (unrelated to this change): Codex sessions refused with HTTP 422, cursor unchanged.
5. The Kali VM ran out of disk during a build (ENOSPC); this session's superseded build copies and test homes were removed (5.3 GB free after).

Follow-up (same day): finding 1 fixed. `apps/mcp-server/src/detach.rs` (Windows) starts the watcher with `CreateProcessW` and `bInheritHandles=FALSE`, keeping the flags (`CREATE_BREAKAWAY_FROM_JOB` first, then `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW`); Unix keeps std `Command` with null stdio. Test `a_piped_starter_is_not_held_open` (MCP unit 79). Observed: `install-hooks` run with its output captured through a PowerShell pipe returned in 2.1 s while the watcher it started kept running; the previous binary hung until that watcher was stopped. This also covers the SessionStart hook (`brief --auto`), which starts the watcher the same way. The archives from `c7937d0` predate this fix and the rebase onto main `6459e80`; rebuild them before publishing.

### Clean Windows: alpha.3 cannot start PostgreSQL; Visual C++ runtime shipped — 2026-11-28

Observed on a clean Windows 10 Enterprise VM (build 19044, Hyper-V, 2.2 GB RAM; no AIDrive, Claude Code, Codex, Node or git; only the 2017 Visual C++ runtime in System32, no `vcruntime140_1.dll`): the published `v0.0.1-alpha.3` setup archive downloaded in 6 s and matched `SHA256SUMS` (`e081d073…49878`, payload `1413a1d1…` as in its BUILD file, source `088df240`). `install.cmd` from a new project folder then failed: "AIDrive did not become ready"; the API log held only `Command error: stdout=; stderr=`. The bundled `initdb.exe` exits with `0xC0000135` (DLL not found): the PostgreSQL 17 build links the Visual C++ 2015-2022 runtime. With `vcruntime140.dll`, `vcruntime140_1.dll` and `msvcp140.dll` placed in PostgreSQL's `bin` folder, `initdb` and the server ran and `aidrive up` was ready in 8 s. So alpha.3 does not work on a Windows without that runtime, and the installer said nothing useful.

Fix (`c16d518`): the Windows archive carries those three DLLs from the Visual Studio redistributable folder (`VC\Redist\MSVC\14.44.35112\x64\Microsoft.VC143.CRT`; app-local, no admin; notice in THIRD-PARTY-NOTICES.md); the API copies them into PostgreSQL's `bin` before setup and, on a first start where the folder only appears during setup, retries a failed setup once after copying; a remaining failure names the missing DLLs and the Visual C++ Redistributable. `bundle-release.py` refuses a Windows payload without them.

Observed with the setup archive built from `c16d518` (`a882b111ccc3e6893343502cfb9c76ae3f2941a29e95643420e0b9a7065fccf3`, payload `f8b98d9a…2352`) on the same VM after removing the earlier install, data, profiles and PATH entry: `install.cmd` 20.7 s, ready in 15 s, `/` 200, `/api/readyz` ready; the running `postgres` loads `VCRUNTIME140.dll`, `VCRUNTIME140_1.dll` and `MSVCP140.dll` 14.44 from its own `bin` folder; System32 still has no `vcruntime140_1.dll`.

Also seen, not fixed: with no client on PATH the installer ends with "This computer has no AIDrive device key yet … AIDrive could not open the web app"; on a machine where the person installs AIDrive before Claude Code or Codex, the first thing they see is a failure line. Not covered yet: Claude Code and Codex on the VM (the two-client scenario), Linux, and a release built by the GitHub workflow (the runner's VS redistributable path is assumed, not observed).
