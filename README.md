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

Phase 2.3C adds a portable foreground scheduler. It waits efficiently for the persisted local schedule and invokes the same Update Record operation as the manual command. It has no installer, startup-at-login integration, automatic retry, or cross-process lock. See [the Phase 2 guide](docs/phase-2/README.md), [scheduler lifecycle](docs/phase-2/SCHEDULER_LIFECYCLE.md), and [service API](docs/phase-2/SERVICE_API.md).
