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

The default list is a compact factual Git view. To inspect one complete
factual record, obtain its event ID through the explicit raw list:

```sh
cargo run -p schomburg-cli -- events --db /Users/ketones/Documents/projects/schomburg/.schomburg/dev.sqlite3 --raw
```

Then inspect the event:

```sh
cargo run -p schomburg-cli -- event '<event-id>' --db /Users/ketones/Documents/projects/schomburg/.schomburg/dev.sqlite3
```

The default event view is a detailed factual Git presentation. Add `--raw` to
either `events` or `event` to inspect the complete stored record, including
technical identifiers, metadata, and exact raw commit evidence.

Running the import command again reports duplicate commit events; it never
overwrites the existing records.

## Machine-level discovery proof

Discovery is not collection: supported connectors find only candidate sources.
Consent is required before any evidence import. Approval and decline persist in
the selected local database across restarts; rediscovery preserves a decline.
Pause preserves consent, while disconnect stops future collection only. Neither
operation deletes existing evidence. The CLI consent flow is temporary until a
native settings experience exists.
