# Architecture

## Purpose and boundary

Schomburg is a local-first evidence engine. Its responsibility is limited to:

- collecting evidence;
- preserving evidence; and
- presenting evidence.

Schomburg does not summarize, interpret, or infer. AI systems may consume its
evidence, but are outside the engine.

## Workspace

The repository is a Cargo virtual workspace using Rust edition 2024 and Cargo
resolver 3. [`rust-toolchain.toml`](rust-toolchain.toml) tracks the current
stable Rust toolchain for development.

| Crate | Responsibility | Allowed dependencies |
| --- | --- | --- |
| `schomburg-core` | Domain model for preserved evidence | Rust standard library only |
| `schomburg-cli` | Future command-line entry point | `schomburg-core` only, at this stage |

No MSRV is declared. Development uses the current stable Rust toolchain. An
explicit MSRV policy must be selected before the first public package release;
until then, the project does not claim compatibility with older Rust versions.

## Domain model boundary

An `Event` is the immutable record of one observed fact from one source at one
point in time. It is the core evidence record; there is no separate `Evidence`
container or grouping model.

Each event holds a stable identifier, occurred and capture timestamps, source
provenance, event kind, an optional context reference, source-specific payload,
and a schema version. Grouping, filtering, and presentation are views over
events and must not change an underlying event. Corrections and classifications
are separate future concepts and must not rewrite observed evidence.

The core model retains source-specific data without interpreting it. It uses
opaque, strongly typed identifiers and labels; their generation, validation,
and serialization formats are intentionally not decided here.

## Connector boundary

Connectors are outside `schomburg-core`. A connector will translate an external
system's record into an `Event` using a stable `ConnectorId` and source
reference. Adding a connector must therefore not require changes to the core
crate's domain types.

## Decision process

Capture durable, consequential choices as Architecture Decision Records (ADRs)
in [`docs/adr/`](docs/adr/README.md). The foundational decisions above are
recorded in [ADR 0001](docs/adr/0001-event-as-evidence-record.md).
