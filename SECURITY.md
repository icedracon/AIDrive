# Security policy

AIDrive is early software (0.x). It is designed to run on your own computer; the API binds to `127.0.0.1` and local trust only works on a loopback bind.

## Reporting a vulnerability

Please do not open a public issue for a security problem. Use GitHub's private vulnerability reporting on this repository ("Security" → "Report a vulnerability"). Include the version (`aidrive-api --version` is not available yet; give the commit), your OS, and steps to reproduce. You should get an answer within 7 days.

In scope: anything that lets an assistant, a web page or another local user read or change a workspace it was not granted; bypass owner approval; forge content that the next assistant reads as approved (see [docs/research/mcp-security-review.md](docs/research/mcp-security-review.md)); or leak keys, passwords or captured session text.

## What AIDrive does and does not protect against

- Assistant keys are stored as SHA-256 hashes; profiles with the raw key live in your user config folder (`%APPDATA%\AIDrive` or `~/.config/AIDrive`).
- With `AIDRIVE_LOCAL_TRUST=1`, any program running as your OS user can act as the owner. That is the same boundary as your files; do not enable it on a shared or hosted machine.
- The embedded database (`aidrive-api --embedded-db`) keeps its superuser and application passwords in the data folder (`%LOCALAPPDATA%\AIDrive\postgres` or `~/.local/share/AIDrive/postgres`). On Unix the folder is set to 0700 and the password files to 0600 on every start, and the API reaches PostgreSQL through a socket in that folder; on Windows it relies on the user-only ACL of `%LOCALAPPDATA%` (not separately tested). Authentication is `scram-sha-256`, so passwords are not sent in clear text; the loopback connection has no TLS. A server already running on the port is reused only if it serves this data folder. `--embedded-db` refuses a non-loopback `AIDRIVE_API_ADDR`.
- File content, handoffs, messages and captured sessions are written by people and other assistants. They are data, not instructions; a connected assistant can still be misled by them. Known open issues are tracked in [docs/research/mcp-security-review.md](docs/research/mcp-security-review.md).
- Content already delivered to an AI provider cannot be recalled by revoking a key.

The threat model is in [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).
