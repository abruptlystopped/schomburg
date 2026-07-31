# ADR 0003: Persist evidence and annotations in append-only SQLite storage

- Status: Accepted
- Date: 2026-07-31
- Deciders: Schomburg project
- Technical area: Local persistence

## Context and problem statement

Schomburg needs a local persistence layer that preserves immutable observed
events and append-only organizational annotations without interpreting either.
The core domain crate must remain independent of storage technology.

## Decision drivers

- Provide a reliable embedded local database without a server process.
- Preserve every model field through a write/read round trip.
- Prevent normal update, delete, and overwrite operations.
- Support versioned schema evolution.
- Keep future current-state derivation out of storage.

## Considered options

1. SQLite in a dedicated `schomburg-store` crate.
2. Add persistence concerns to `schomburg-core`.
3. Use a file format without transactional migrations or referential integrity.

## Decision outcome

Chosen option: **SQLite in a dedicated `schomburg-store` crate**. The store
depends on `schomburg-core`; core has no SQLite dependency. The store uses
`rusqlite` with bundled SQLite for a consistent local embedded engine.

Its public repository API is append-only: append events and annotations, get by
stable ID, and list in deterministic chronological order. It has no normal
update or delete method. Duplicate IDs use `INSERT ... ON CONFLICT DO NOTHING`
followed by explicit duplicate errors; no record is overwritten.

## Schema and migration strategy

The initial versioned migration creates:

- `schema_migrations` for applied migration versions;
- `events` for immutable observed records; and
- `annotations` for append-only organizational records.

Migrations run transactionally when a store opens. Future changes add a new
ordered migration rather than modifying an already-applied migration.

`annotations.event_id` references `events.id`; an append returns
`MissingTargetEvent` when it is absent. `supersedes_annotation_id` references
`annotations.id`; an append returns `MissingSupersedesAnnotation` when it is
absent. Foreign keys are enabled as a database backstop.

The append boundary additionally requires a superseding annotation to target
the same event and use the same organizational field as its predecessor. It
rejects cross-event links, cross-field links, and self-supersession before any
insert occurs. The predecessor remains unchanged and queryable.

## Serialization and ordering

Event payload bytes are stored directly as SQLite BLOBs. Source metadata is a
JSON object from the model's ordered string map; decoding restores the exact
key/value strings without assigning semantics. Annotation values use separate
`value_kind` and `value_text` columns, preserving the distinction between
`Context(ContextId)` and `Opaque(String)`.

Timestamps use a 13-byte binary encoding: a pre-/post-epoch marker, unsigned
seconds, and nanoseconds. Pre-epoch duration bytes are inverted so SQLite BLOB
ordering is chronological. This avoids narrowing `SystemTime` to one signed
nanosecond integer. Events list by `occurred_at, id`; annotations list by
`assigned_at, id`.

## Consequences

### Positive

- Storage is local, transactional, and independent of a server process.
- Events and annotations round-trip without losing payload bytes, metadata, or
  typed context references.
- Referential history remains intact after supersession.
- Ordering is deterministic even when timestamps tie.

### Negative

- The store adds native SQLite build work through the bundled dependency.
- JSON metadata is a compatibility contract for future readers and migrations.
- Consumers must still implement future validity and current-state selection.

### Neutral / follow-up

- Context tables, derived current-assignment tables, search, filters, and views
  are not part of this schema.
- Metadata schema evolution, concurrency policy, backups, encryption, and
  database-location policy remain undecided.
- Malformed or unsupported stored representations return explicit errors rather
  than being repaired or interpreted by the store.

## Validation

The design is validated by temporary-database tests for migration, exact round
trips, duplicates, ordering, reopening, typed context values, referential
integrity, and absence of update/delete APIs.
