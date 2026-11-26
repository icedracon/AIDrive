# Initial threat model

## Assets

- User files and extracted text
- Workspace membership and access decisions
- Approved memories, source references and AI connection secrets

## Main threats

- Cross-workspace reads or writes
- Path traversal or unsafe file replacement
- Prompt injection embedded in uploaded documents
- Leakage of provider credentials or private document content
- Unauthorized or misleading memory persistence

## Initial controls

- Enforce workspace authorization at the service and database layers.
- Generate storage object keys; do not map raw filenames to filesystem paths.
- Treat document text as data, not executable instructions.
- Keep secrets outside the database in plaintext and outside logs.
- Require user approval before saving durable memory.

This is a starting checklist, not a completed security assessment.
