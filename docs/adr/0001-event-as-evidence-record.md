# ADR 0001: Model each event as one immutable evidence record

- Status: Accepted
- Date: 2026-07-31
- Deciders: Schomburg project
- Technical area: Core domain model

## Context and problem statement

Schomburg must preserve observed evidence without interpreting it. The core
model needs a stable unit of preservation while avoiding an early commitment to
grouping, storage, or connector implementations.

## Decision drivers

- Preserve one observed fact together with its provenance.
- Keep grouping and presentation from changing evidence.
- Allow new sources without changing the core domain model.
- Avoid interpretation and inferred meaning in the engine.

## Considered options

1. Use `Event` as the immutable record of one observed fact.
2. Introduce a separate `Evidence` container that groups events.

## Decision outcome

Chosen option: **Use `Event` as the immutable record of one observed fact**.
An event carries only capture facts: provenance, timing, kind, payload, and
schema version. No separate evidence container is introduced. Organizational
metadata is explicitly outside the event and is governed by ADR 0002.

## Consequences

### Positive

- The preserved unit is clear and directly attributable to a source.
- Future grouping and presentation can remain views over immutable events.
- Connectors can map their external records into one stable core type.

### Negative

- Relationships, corrections, and classifications require separate domain
  models.
- The generic payload intentionally does not provide source-specific semantics.

### Neutral / follow-up

- Event identifier formats, serialization, storage, and connector interfaces
  remain undecided.

## Validation

The model remains appropriate if a new source can create an event without
changing `schomburg-core` domain types and views can group events without
rewriting them.
