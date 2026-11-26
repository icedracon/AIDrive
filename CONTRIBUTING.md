# Contributing

Thanks for helping. Read [AGENTS.md](AGENTS.md) first: the same rules apply to people and to AI assistants working on this repository.

## Ground rules

- One scoped change per pull request, with a reproducible check.
- Never report a feature as working until its data path works end to end; say what you verified and what you did not.
- Every workspace query and mutation is scoped to an authorized workspace (row-level security in PostgreSQL plus API checks).
- No secrets in source, logs, fixtures or example data. Dependencies stay pinned through `Cargo.lock` and `package-lock.json`.
- No code comments unless the reason is not obvious.

## Checks before a pull request

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
# Linux/WSL with PostgreSQL 17+ server tools; each creates its own private cluster:
cargo build --locked -p aidrive-db && bash scripts/db-check.sh
bash scripts/api-check.sh
# web
cd apps/web && npm ci && npm run lint && npm test && npm run build
```

Schema changes are new numbered files in `migrations/`; never edit an applied migration.

## License

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in AIDrive by you, as defined in the Apache-2.0 license, shall be dual licensed as below, without any additional terms or conditions.

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
