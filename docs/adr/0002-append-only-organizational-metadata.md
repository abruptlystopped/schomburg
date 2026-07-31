# ADR 0002: Preserve organizational metadata as append-only history

- Status: Accepted
- Date: 2026-07-31
- Deciders: Schomburg project
- Technical area: Core domain model

## Context and problem statement

Observed evidence must remain immutable, while organizational metadata such as
context, lens, category, tag, or visibility may change. Storing an authoritative
current value on an event would overwrite history and conflate observed facts
with later organization.

## Decision drivers

- Preserve observed facts unchanged.
- Retain every organizational change as queryable history.
- Support multiple kinds of organizational assignment without embedding their
  semantics in the core.
- Permit later presentation to choose a current value without storing one on an
  event.

## Considered options

1. Store current organizational fields directly on `Event`.
2. Store immutable, append-only annotations that target an `Event`.

## Decision outcome

Chosen option: **Store immutable, append-only annotations that target an
`Event`**. Each annotation records a stable ID, target event, opaque field and
value, assignment source, timestamp, optional superseded annotation, and schema
version. A context value is a typed `ContextId`, reserved for a future
first-class context model; other value domains remain opaque.

An annotation may supersede a prior annotation, but it does not alter that
prior record. Current state is not stored on `Event`; future presentation will
derive it by selecting the latest valid assignment history.

## Consequences

### Positive

- Original evidence cannot be rewritten by organizational changes.
- Earlier and later assignments remain independently queryable.
- New assignment types do not require new core fields or enums.

### Negative

- Consumers need a future selection rule to present a current value.
- Storage must preserve ordering and relationships between annotations.

### Neutral / follow-up

- Validity rules, conflict handling, ordering tie-breakers, persistence, and
  presentation are not defined by this ADR.
- A source-provided context hint may be retained as event payload, but is not an
  authoritative assignment.
- Confidence is omitted until a numeric scale and validation policy are agreed;
  free-form confidence values would not be comparable.

## Validation

The model remains appropriate if a later annotation can supersede an earlier
one without modifying either event capture data or the earlier annotation.
