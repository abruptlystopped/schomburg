# Roadmap

## Status

This is a planning scaffold, not a commitment or schedule.

## Phase 1 — Architecture and developer experience

- [x] Define the engine boundary: collect, preserve, and present evidence only.
- [x] Define the initial workspace members and their dependency direction.
- [x] Define the immutable event domain model and connector boundary.
- [x] Record the foundation decisions as an ADR.
- [ ] Select an MSRV policy before the first public package release.

## Next phase — Implementation planning

- [ ] Define the first evidence-collection workflow without expanding the
  engine's responsibility.
- [ ] Define preservation and presentation requirements for that workflow.
- [ ] Establish acceptance criteria before implementing it.

## Open planning questions

- What is the first evidence source Schomburg should support?
- What retention, export, and presentation requirements apply to that source?
- Which Rust version will become the public MSRV policy?
