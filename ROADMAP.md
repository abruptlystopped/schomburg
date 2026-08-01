# Roadmap

## Status

This is a planning scaffold, not a commitment or schedule.

## Phase 1 — Architecture and developer experience

- [x] Define the engine boundary: collect, preserve, and present evidence only.
- [x] Define the initial workspace members and their dependency direction.
- [x] Define the immutable event domain model and connector boundary.
- [x] Separate immutable observed evidence from append-only organizational
  metadata history.
- [x] Add append-only local SQLite persistence for events and annotations.
- [x] Define storage-independent connector and engine contracts.
- [x] Import historical commits from one local Git repository.
- [x] Provide explicit-path CLI commands for local Git import and factual event
  inspection.
- [x] Define connector-owned, structured factual presentation for stored
  evidence.
- [x] Establish machine-level discovery, consent, persistent connections, and a run-once agent lifecycle for supported connectors.
- [x] Record the foundation decisions as an ADR.
- [ ] Select an MSRV policy before the first public package release.

## Next phase — Implementation planning

- [ ] Define the first evidence-collection workflow without expanding the
  engine's responsibility.
- [ ] Define preservation and presentation requirements for that workflow.
- [ ] Define how presentation selects the latest valid organizational
  assignment while retaining history.
- [ ] Define a numeric confidence scale and validation policy before adding
  confidence to annotations.
- [ ] Establish acceptance criteria before implementing it.

## Open planning questions

- What is the first evidence source Schomburg should support?
- What retention, export, and presentation requirements apply to that source?
- Which Rust version will become the public MSRV policy?
