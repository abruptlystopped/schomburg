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
| `schomburg-connector-git` | Historical local Git commit import | `schomburg-core`, `schomburg-connector`, bundled libgit2 |
| `schomburg-engine` | Connector lifecycle, validation, and event acceptance | `schomburg-connector`, `schomburg-core`, `schomburg-store` |
| `schomburg-store` | Local SQLite persistence for core records | `schomburg-core`, SQLite, JSON serialization |
| `schomburg-cli` | Explicit-path local proof commands | Core, connector, Git connector, engine, and store crates |

No MSRV is declared. Development uses the current stable Rust toolchain. An
explicit MSRV policy must be selected before the first public package release;
until then, the project does not claim compatibility with older Rust versions.

The CLI requires explicit database and repository paths for the local proof.
Only `init` creates the specified database parent directory. It does not choose
or write to a global user data directory.

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
No connector assigns organizational metadata, derives current state, or
interprets collected evidence. See
[ADR 0004](docs/adr/0004-connector-contract-and-engine-boundary.md).

## Machine-level discovery and consent

`schomburg-agent` is a portable run-once lifecycle for connector discovery,
persistent consent, and approved collection. Discovery is not collection:
connector extensions provide opaque candidates, while only approved enabled
connections run through connector → engine → store. The engine remains
source-agnostic; connectors define what can be discovered. Contexts remain
separate. Pause and disconnect affect future collection only and never delete
preserved evidence. CLI consent is temporary pending native settings.

Reconciliation configuration is separate mutable operational state: monitoring
enabled/paused, Record Folder, schedule, local time, next eligible run, and
last status fields. It is not evidence and is never stored in Events. Phase
2.3A calculates eligibility only; `reconcile_once` and a scheduler lifecycle
remain deferred.

## Portable service boundary

`schomburg-service` is the portable, structured control surface for the CLI,
future macOS menu-bar shell, Windows tray shell, and other shells. It opens the
store, registers supported connectors and presenters, and coordinates the
agent, presenter, and operational configuration. It does not own Events or
Annotations, parse Git payloads, or duplicate engine or store logic. Shells use
its structured status and result types; they must not parse CLI output.

The service prevents overlapping Update Record operations within one service
instance. Cross-process locking, platform-specific folder opening, and the
long-running scheduler lifecycle remain deferred. macOS will be the first
shell, not the owner of Schomburg; Windows will use this same boundary.

Connectors also own factual presentation of the events they produce. The shared
contract returns structured compact and detailed presentation data, rather than
preformatted terminal text. A host routes a stored event to a presenter by its
connector provenance, then renders that data for its terminal or UI. The engine
and store do not know Git or any other source format. Presenters must reject
events outside their provenance or supported event kinds; raw evidence remains
available only through an explicit debug path. Presentation selects and labels
source facts, but never summarizes or interprets them. See
[ADR 0006](docs/adr/0006-connector-owned-factual-presentation.md).

## Git historical import

`schomburg-connector-git` is the first production connector. It imports only
commits reachable from the repository's current `HEAD`, in oldest-first
topological order. It does not monitor repositories, track files or diffs, or
represent branches as evidence.

One Git commit becomes one `git.commit` event. Its source payload contains the
exact raw Git commit object bytes, which preserve commit hash, parent hashes,
author and committer identities/timestamps, and complete commit message without
interpretation. `occurred_at` uses the Git committer timestamp; author time and
the committer's timezone remain in the raw payload. `captured_at` is set when
Schomburg imports the commit.

Repository identity is the canonical local Git-directory path represented as
native path bytes encoded in hexadecimal. Event IDs derive from connector
namespace, repository identity, and commit hash. Reimporting a repository is
therefore deduplicated by the append-only store; identical commit hashes in
different repository identities remain distinct. Moving a repository changes
its identity and produces different event IDs. Rewritten commits no longer
reachable from `HEAD` are not newly imported, while previously captured events
remain preserved. See [ADR 0005](docs/adr/0005-git-historical-commit-import.md).

The Git connector presents compact records using the first commit-message line,
short commit hash, repository display name, and committer time. Its detailed
record retains the full message, full hash, stored repository reference, both
Git identities/timestamps/timezones, and parent hashes; the exact raw commit
object remains available on the CLI's `--raw` path.

## Decision process

Capture durable, consequential choices as Architecture Decision Records (ADRs)
in [`docs/adr/`](docs/adr/README.md). The foundational decisions above are
recorded in [ADR 0001](docs/adr/0001-event-as-evidence-record.md) and
[ADR 0002](docs/adr/0002-append-only-organizational-metadata.md), and
[ADR 0003](docs/adr/0003-sqlite-append-only-persistence.md), and
[ADR 0004](docs/adr/0004-connector-contract-and-engine-boundary.md), and
[ADR 0005](docs/adr/0005-git-historical-commit-import.md), and
[ADR 0006](docs/adr/0006-connector-owned-factual-presentation.md).
