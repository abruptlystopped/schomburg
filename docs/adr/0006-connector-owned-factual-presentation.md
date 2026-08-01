# ADR 0006: Connectors own factual presentation of their evidence

- Status: Accepted
- Date: 2026-07-31
- Deciders: Schomburg project
- Technical area: Evidence presentation architecture

## Context and problem statement

Stored events are source-agnostic core records, but factual fields differ by
source. A terminal or future UI needs concise and detailed views without making
the engine understand Git payloads or introducing a second source-aware engine.

## Decision outcome

`schomburg-connector` defines `EventPresenter`, implemented by connector-owned
code. It returns structured `CompactPresentation` and `DetailedPresentation`
values, ordered `PresentationField` values, and optional exact `RawEvidence`.
It does not return terminal-ready text. A host-owned `PresentationRegistry`
routes an event by its connector provenance to a registered presenter.

Each presenter verifies both connector provenance and supported event kind. It
returns an explicit error for a mismatch, unsupported kind, malformed payload,
missing factual field, or invalid timestamp. The engine and store have no
presentation dependency and no knowledge of Git or another source.

Compact data is for lists and timelines; detailed data is for one-event
inspection. Both are factual renderings of preserved evidence. Raw evidence is
available for an explicit debug path, not required in normal output.

## Consequences

- New sources bring their own factual presentation without changing the engine.
- Different hosts can render the same structured fields without parsing payloads.
- Presentation cannot create current state, context, classification, summary,
  intent, importance, or other interpretation.
- Hosts must register an appropriate presenter to display a source's events.

## Git application

The Git presenter compactly exposes the first message line, short hash,
repository display name, and the event's committer timestamp. Its detailed
fields expose the complete message, full hash, stored repository reference,
author/committer names, emails, timestamps and timezones, and parent hashes.
The raw Git commit object stays available unchanged.

## Deferred decisions

Cross-source display style, localization beyond host timestamp rendering,
presentation plugin discovery, and UI layout are deferred. They must not add
semantics to factual evidence.
