# Roadmap

## Status

This is a planning scaffold, not a commitment or schedule.

## Phase 1 Alpha — Complete

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

## Phase 2 — Presentation

- [ ] Make preserved evidence usable for ordinary people without terminal commands.
- [ ] Provide a native or desktop presentation surface with readable daily timeline, date navigation, source/repository grouping, and compact/detailed views.
- [ ] Provide connection settings, consent UI, global monitoring control, pause controls, and monitoring schedules.

### Phase 2.3A — Persistent reconciliation configuration and scheduling math — Complete

- [x] Persist monitoring, Record Folder, schedule/time, next eligible run, and status fields.
- [x] Support Daily, Weekdays, and selected-weekday policies.

### Phase 2.3B — Reconcile once

- [x] Implement reusable collection and affected-date record generation.

### Phase 2.3C — Scheduler lifecycle

- [x] Implement portable long-running lifecycle, bounded waiting, cancellation,
  and one missed-run catch-up policy.
- [ ] Add automatic retry and cross-process coordination (deferred).

Native macOS menu-bar and Windows tray shells remain later work.

### Phase 2.4 — Portable Service API — Complete

- [x] Add the structured shared control API for the CLI and future shells.
- [x] Route user-level CLI configuration, discovery, consent, and connection
  commands through the service.
- [x] Prevent overlapping Update Record calls within one service instance.

Cross-process locking and scheduler lifecycle remain deferred.

## Open planning questions

- What presentation workflow best serves ordinary users?
- What retention and export requirements apply?
- Which Rust version will become the public MSRV policy?
