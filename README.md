# Schomburg

Schomburg is a local-first evidence engine. It collects, preserves, and
presents evidence. It does not summarize, interpret, or infer.

AI systems are consumers of Schomburg's evidence, not part of the engine.

## Start here

- [Project brief template](docs/templates/project-brief.md)
- [Architecture](ARCHITECTURE.md)
- [Roadmap](ROADMAP.md)
- [Documentation index](docs/README.md)

## Repository layout

```text
.
├── crates/             # Future Rust workspace members
├── docs/               # Project documentation and templates
├── ARCHITECTURE.md     # Architecture context and decision log pointers
├── Cargo.toml          # Empty virtual Rust workspace
└── ROADMAP.md          # Planning template
```

## Current status

The workspace currently contains only `schomburg-core`, the dependency-free
domain model, and `schomburg-cli`, an empty future command-line entry point.
