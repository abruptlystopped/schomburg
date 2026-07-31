# Architecture Decision Records

ADRs capture decisions that are consequential, difficult to reverse, or useful
for future contributors to understand.

## Workflow

1. Copy [`0000-template.md`](0000-template.md).
2. Rename it to the next four-digit number and a short kebab-case title, for
   example `0001-example-decision.md`.
3. Set its status to `Proposed` while it is under discussion.
4. Update the status to `Accepted`, `Rejected`, `Superseded`, or `Deprecated`
   when its outcome is known.
5. Link superseding records in both directions when applicable.

ADRs describe the context, decision, and consequences. They should not be used
to silently introduce implementation details before the decision is made.
