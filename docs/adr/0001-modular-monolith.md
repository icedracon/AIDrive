# ADR 0001: Start with a modular monolith

## Status

Accepted for the prototype.

## Decision

Keep the API, MCP server, persistence and provider integrations as separate Rust workspace packages or modules in one repository and deployable product. Use PostgreSQL and local object storage in development.

## Reason

The first milestone needs to prove the end-to-end product loop with low operational cost. Service boundaries can be extracted later if real scaling or ownership needs appear.

## Consequences

- Shared domain types can stay versioned together.
- Modules need clear dependency direction and workspace authorization rules.
- No microservice deployment or service mesh is needed for the prototype.
