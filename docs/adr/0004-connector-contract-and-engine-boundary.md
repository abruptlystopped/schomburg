# ADR 0004: Separate connector contracts from engine-owned persistence

- Status: Accepted
- Date: 2026-07-31
- Deciders: Schomburg project
- Technical area: Evidence collection architecture

## Context and problem statement

Schomburg will collect evidence from future external sources, but connectors
must not interpret evidence or write directly to SQLite. The project needs a
stable contract that permits source-specific implementations without making the
core model or connectors depend on persistence technology.

## Decision drivers

- Keep connector implementations independent of SQLite and storage mechanics.
- Require all persisted events to pass through one engine-owned acceptance path.
- Preserve immutable event provenance.
- Permit connector capabilities to grow without hardcoding source categories.
- Keep connector ownership and lifecycle explicit.

## Considered options

1. Let connectors depend on and append directly to `schomburg-store`.
2. Put connector contracts and engine orchestration in `schomburg-core`.
3. Use a storage-independent connector-contract crate and a separate engine
   crate that owns validation and persistence.

## Decision outcome

Chosen option: **Use `schomburg-connector` for storage-independent contracts
and `schomburg-engine` for lifecycle, validation, and persistence**.

`schomburg-connector` depends only on `schomburg-core`. It defines:

- `Connector`, which emits immutable events;
- connector-owned factual presentation contracts (detailed in ADR 0006);
- `EventSink`, an engine-owned event destination;
- immutable connector descriptors and extensible, opaque capabilities;
- descriptor registration; and
- connector-facing errors that expose no SQLite types.

`schomburg-engine` owns a descriptor registry and receives host-owned connector
instances for a collection run. It verifies registration and descriptor match,
then scopes an event sink to the running connector ID. The sink rejects any
event whose source provenance ID does not match and otherwise calls the
append-only store API.

## Lifecycle and ownership

1. A host constructs and owns a connector implementation.
2. The host registers its immutable descriptor with the engine.
3. The host passes the connector by mutable reference for a collection run.
4. The connector sends immutable events to the supplied sink.
5. The engine validates provenance and appends accepted events.

The registry owns descriptor metadata only. It does not own, discover, start,
or stop connector implementations. Connector scheduling, retry, configuration,
and source-specific lifecycle policy are deferred.

## Consequences

### Positive

- Connectors cannot receive a SQLite store through their required contract.
- All accepted events follow one provenance-validation and append path.
- New connectors can depend on core and the contract crate only.
- Capability identifiers remain extensible without encoding interpretation.

### Negative

- A host must explicitly register and invoke each connector.
- Storage failures cross the sink boundary as generic persistence messages,
  without exposing SQLite error types.

### Neutral / follow-up

- The first production connector is the historical Git importer specified by
  ADR 0005.
- Connector configuration, scheduling, cancellation, retries, concurrency,
  capability semantics, and event-level validation beyond connector provenance
  remain undecided.
- The engine does not derive current state or interpret collected data.
- The engine does not route or render source-specific presentation.

## Validation

Contract tests use a test-only connector to verify registration/capabilities,
successful engine append, unregistered-connector rejection, and provenance
mismatch rejection without persistence.
