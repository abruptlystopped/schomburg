# Schomburg

Schomburg is a local-first evidence engine that preserves factual evidence from supported local tools.

**Alpha status:** Phase 1 is complete for local macOS development. Git is the only production connector. The temporary CLI supports discovery, consent, collection, and factual inspection; no installer, stable compatibility promise, or formal MSRV exists yet.

The core keeps immutable Events and append-only Annotations. Connectors collect and factually present their own evidence. The engine validates provenance and stores source-agnostic records; the agent coordinates discovery and approved connections.

Start with the [Phase 1 guide](docs/phase-1/README.md), [first-run instructions](docs/phase-1/FIRST_RUN.md), and [architecture](ARCHITECTURE.md).

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The next proposed tag is `v0.1.0-alpha.1`; it has not been created.

Phase 2.3A adds persisted reconciliation configuration and portable scheduling math. It does not yet run reconciliation automatically or start a scheduler process.
