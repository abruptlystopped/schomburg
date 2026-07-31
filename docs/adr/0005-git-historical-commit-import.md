# ADR 0005: Import local Git commit history through a storage-independent connector

- Status: Accepted
- Date: 2026-07-31
- Deciders: Schomburg project
- Technical area: Git evidence collection

## Context and problem statement

Schomburg needs its first production evidence connector: a local historical Git
commit importer. It must capture factual commit evidence without assigning
meaning, depending on SQLite, or monitoring future changes.

## Decision drivers

- Preserve complete commit facts, including arbitrary message bytes.
- Avoid a requirement that a `git` executable be installed on every target.
- Keep connector dependencies independent of storage and engine crates.
- Give repeated imports deterministic, append-only behavior.
- Keep the first end-to-end scope small and reproducible.

## Considered options

1. Invoke the installed `git` executable and parse formatted output.
2. Use `git2`/libgit2 for structured local repository access.

## Decision outcome

Chosen option: **Use `git2` with bundled libgit2**. It accesses local Git
objects through a Rust API, does not require an external Git command, and can
copy exact raw commit-object bytes into event payloads. The trade-off is native
library build weight. Network, SSH, GitHub, and remote operations are not
enabled or used.

The import scope is commits reachable from the repository's current `HEAD`,
walked in oldest-first topological order. This covers one current history view
without treating refs or branches as separate evidence.

Each commit becomes one immutable `git.commit` event. Its payload is the exact
raw Git commit object, with metadata identifying its commit hash, canonical
repository reference, and payload format. The object preserves parent hashes,
author name/email/timestamp, committer name/email/timestamp, and complete
commit message. `occurred_at` is the committer timestamp; `captured_at` is the
local import time. Author time and timezone details remain in the raw payload.

## Repository identity and repeat imports

Repository identity is the canonical Git-directory path. Native path bytes are
hex-encoded into a stable reference, resolving symlinks and lexical path
differences before identity is formed. Each event ID is derived deterministically
from `git:commit`, that repository reference, and the commit hash.

Consequently, a reimport of the same repository emits the same ID and the
engine/store reports it as a duplicate without overwriting evidence. The same
commit hash in distinct canonical Git directories produces distinct event IDs.
Moving a repository changes its canonical identity, so it produces new event
IDs rather than attempting to infer that the repositories are equivalent.
Commits removed by rewritten history are not reachable from later `HEAD` walks;
previously preserved evidence is not deleted.

## Error and reporting model

The connector reports explicit path-missing, not-repository, libgit2/filesystem,
malformed-commit, and event-acceptance errors. It records imported, duplicate,
rejected, and failed counts for its last import attempt. To support exact
duplicate reporting without exposing SQLite, the connector contract distinguishes
a duplicate event ID from a generic persistence failure.

## Consequences

### Positive

- Commit evidence is copied without lossy pretty-format parsing.
- Connector code depends on no store, engine, or SQLite crate.
- Repeat imports are deterministic and append-only.
- Source data retains all available commit facts without classification.

### Negative

- Bundled libgit2 increases build time and native dependency surface.
- Path-based identity does not survive repository moves.
- The first scope does not import commits reachable only from non-HEAD refs.

### Neutral / follow-up

- Live monitoring, file changes, diffs, patches, branches as evidence, remote
  hosting integration, and repository equivalence are out of scope.
- Bare repositories and non-UTF-8 commit payloads are preserved by raw bytes;
  non-Unix/non-Windows path-identity fallback is lossy and should be revisited
  before supporting such platforms.
- Commit-walk tie-breaking across complex graphs is delegated to libgit2's
  topological walk; only the documented `HEAD` scope is guaranteed here.

## Validation

Temporary repositories verify factual payload fidelity, timestamp choice,
deterministic linear history import, distinct repository identities, explicit
path errors, connector provenance, and reimport through engine and SQLite.
