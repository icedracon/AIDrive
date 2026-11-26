# ADR 0002: Password + server-side sessions for the pilot; passkeys before hosted release

## Status

Accepted for G1 local development, 2026-11-18. Revisit before any hosted pilot.

## Context

G1 requires server-validated identity and a secure session lifecycle. The pilot has one human owner per workspace, runs locally first, and must not accept identity from request-supplied database settings. The API is Rust/Axum with PostgreSQL row-level security; the web app proxies `/api/*` to it from the same origin.

## Options considered

| Option | For | Against |
| --- | --- | --- |
| Argon2id password + opaque server sessions | Few, maintained primitives (RustCrypto `argon2`, `sha2`, `getrandom` already in the lockfile); works offline and on `localhost`; revocation is a row update | Passwords are not phishing-resistant; needs a breached-password list and recovery flow |
| Passkeys (WebAuthn, `webauthn-rs` 0.6.x) | Phishing-resistant; NIST AAL2 requires offering such an option | Pre-1.0, MPL-2.0, server-side ceremony state, account recovery still needs a second path; heavier first slice |
| External OIDC provider | Delegates credential storage and MFA | Requires third-party registration and network access for local development; adds an external dependency the pilot has not authorized |
| Hosted auth SaaS | Fast to integrate | Spend and data residency decisions belong to the founder; identity would live outside the system of record |

## Decision

- Argon2id with OWASP's minimum profile (m = 19 MiB, t = 2, p = 1), PHC string stored in a table the application role cannot read directly.
- Single-factor password rules follow NIST SP 800-63B rev. 4: at least 15 characters, at most 256, no composition rules, no periodic rotation, reject a blocklist (currently a small built-in list plus the account email; a breached-password corpus is pending).
- Session token: 256 bits from the OS CSPRNG, sent only as cookie `__Host-aidrive_session` (`Secure; HttpOnly; SameSite=Strict; Path=/`, no `Domain`). The database stores SHA-256 of the token, never the token. A new token is issued on every login; logout revokes the row server-side.
- Lifetime: absolute 7 days, idle 24 hours. This is within NIST AAL1 reauthentication guidance (30 days) and is deliberately not an AAL2 claim.
- CSRF: every state-changing request must carry `Sec-Fetch-Site: same-origin`, or (when that header is absent) an `Origin` equal to the configured web origin. GET never changes state. `SameSite=Strict` is defense in depth, not the only control.
- Database access for authentication goes through narrow `SECURITY DEFINER` functions (`app.register_user`, `app.login_lookup`, `app.create_session`, `app.resolve_session`, `app.revoke_session`); `aidrive_app` gets no direct table privileges on credentials or sessions.
- Sign-up is disabled unless `AIDRIVE_ALLOW_SIGNUP=1`. There is no implicit local test identity.

## Consequences

- An attacker who obtains `aidrive_app` credentials can mint sessions for any user. RLS protects against application scoping bugs, not a compromised server; credential storage still keeps password hashes out of ordinary table reads.
- Passwords are phishing-susceptible. Before a hosted pilot, add passkeys (or an equivalent phishing-resistant option) and a recovery flow; that work is part of G1's hosted exit, not the local slice.
- Login responses do not reveal whether an email exists; registration still can, through the unique constraint. Rate limiting is not implemented yet.

## Sources (read 2026-11-18)

- OWASP [Session Management Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html): ≥64-bit session entropy (≥128-bit CSPRNG for custom IDs), store a fast hash of random verifiers, `Secure`/`__Host-` cookies, renew on login, invalidate server-side on logout.
- OWASP [Password Storage Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html): Argon2id first; minimum m=19 MiB, t=2, p=1.
- NIST [SP 800-63B rev. 4](https://pages.nist.gov/800-63-4/sp800-63b.html): 15-character minimum for single-factor passwords, no composition rules or periodic rotation, blocklist required, AAL1 reauthentication ≤30 days, AAL2 must offer a phishing-resistant option.
- OWASP [CSRF Prevention Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html): `Sec-Fetch-Site` as primary signal with mandatory `Origin` fallback; no state changes on GET; SameSite alone is insufficient in most deployments.
- [webauthn-rs docs](https://docs.rs/webauthn-rs) 0.6.2: ceremony state must be persisted server-side.
