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
| `schomburg-connector` | Storage-independent connector contracts | `schomburg-core` only |
| `schomburg-engine` | Connector lifecycle, validation, and event acceptance | `schomburg-connector`, `schomburg-core`, `schomburg-store` |
| `schomburg-store` | Local SQLite persistence for core records | `schomburg-core`, SQLite, JSON serialization |
| `schomburg-cli` | Future command-line entry point | `schomburg-core` only, at this stage |

No MSRV is declared. Development uses the current stable Rust toolchain. An
explicit MSRV policy must be selected before the first public package release;
until then, the project does not claim compatibility with older Rust versions.

## Domain model boundary

An `Event` is the immutable record of one observed fact from one source at one
point in time. It is the core evidence record; there is no separate `Evidence`
container or grouping model.

Each event holds only factual capture data: a stable identifier, occurred and
capture timestamps, source provenance (including its connector identifier),
event kind, source-specific payload, and schema version. Source-provided hints
may be retained in that payload but do not become current organizational state.

## Organizational metadata boundary

Organizational metadata is stored separately as immutable, append-only
`Annotation` records. An annotation targets an event and retains its field,
value, assignment source, timestamp, optional superseded annotation, and schema
version.

Annotations may represent context, lenses, categories, tags, visibility, or
other organizational data. Context values use a typed `ContextId` reserved for
the future first-class context model; other values remain opaque. The core does
not interpret them. An event contains no authoritative current context or other
editable organizational state.

A later annotation may supersede an earlier one without changing that earlier
record. Presentation will eventually select the latest valid assignment from
history; the selection rule and persistence are deliberately not implemented in
this phase.

Confidence is intentionally not modeled until the project defines a numeric
scale and its validation rules. The core must not preserve incompatible labels
or source-specific number formats as if they were comparable confidence.

## Local persistence boundary

`schomburg-store` persists `Event` and `Annotation` records in an embedded
SQLite database. It exposes append, get, and list operations only: observed
records and organizational history cannot be updated or deleted through its
normal API.

The store has no interpretation or current-state logic. It preserves event
payload bytes as BLOB data, serializes string metadata as a JSON object, and
stores annotation value kind separately from its text so `ContextId` remains
distinct from an opaque value. Timestamps use a fixed-width sortable binary
encoding, allowing exact `SystemTime` reconstruction and deterministic
chronological ordering.

The store enforces that every annotation targets an existing event and that a
supersedes reference names an existing annotation for the same event and the
same organizational field. An annotation cannot supersede itself. These checks
preserve append-only history without selecting which assignment is current. See
[ADR 0003](docs/adr/0003-sqlite-append-only-persistence.md).

The core model retains source-specific data without interpreting it. It uses
opaque, strongly typed identifiers and labels; their generation, validation,
and serialization formats are intentionally not decided here.

## Connector boundary

Connectors are outside `schomburg-core` and implement the storage-independent
`schomburg-connector` contract. A connector translates external records into
immutable `Event` values using a stable `ConnectorId` and source reference.
Adding a connector must not require changes to core domain types or introduce a
SQLite dependency.

Each connector has an immutable descriptor: its ID and an ordered set of opaque
capability identifiers. The engine registers descriptors but does not own or
construct connector instances. The host owns a connector instance and gives it
to the engine for one collection lifecycle:

1. Register the connector descriptor.
2. Invoke the engine with the host-owned connector.
3. The connector emits events to an engine-owned sink.
4. The engine verifies that every event's provenance ID matches the registered
   running connector and appends it through `schomburg-store`.

Connectors never receive the store or write to it directly. The sink exposes
only event acceptance errors; SQLite-specific errors remain inside the store.
No connector implementation, source-specific capability, current-state logic,
or interpretation is defined by this architecture. See
[ADR 0004](docs/adr/0004-connector-contract-and-engine-boundary.md).

## Decision process

Capture durable, consequential choices as Architecture Decision Records (ADRs)
in [`docs/adr/`](docs/adr/README.md). The foundational decisions above are
recorded in [ADR 0001](docs/adr/0001-event-as-evidence-record.md) and
[ADR 0002](docs/adr/0002-append-only-organizational-metadata.md), and
[ADR 0003](docs/adr/0003-sqlite-append-only-persistence.md), and
[ADR 0004](docs/adr/0004-connector-contract-and-engine-boundary.md).
