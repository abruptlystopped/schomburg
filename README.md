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

## Local Git proof

The CLI provides a local end-to-end proof for importing factual Git commit
evidence and displaying it. It does not monitor repositories or interpret
commits.

The repository-local `.schomburg/` directory is ignored by Git. It is a
deliberate test location, not a decision about the eventual operating-system
data directory.

From this repository, run:

```sh
cargo run -p schomburg-cli -- init --db /Users/ketones/Documents/projects/schomburg/.schomburg/dev.sqlite3
cargo run -p schomburg-cli -- import git --repo /Users/ketones/Documents/projects/schomburg --db /Users/ketones/Documents/projects/schomburg/.schomburg/dev.sqlite3
cargo run -p schomburg-cli -- events --db /Users/ketones/Documents/projects/schomburg/.schomburg/dev.sqlite3
```

Copy an `id` from the `events` output, then inspect the complete factual
record:

```sh
cargo run -p schomburg-cli -- event '<event-id>' --db /Users/ketones/Documents/projects/schomburg/.schomburg/dev.sqlite3
```

Running the import command again reports duplicate commit events; it never
overwrites the existing records.
