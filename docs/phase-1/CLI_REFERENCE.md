# CLI reference

All commands require explicit `--db <path>`.

- `init --db <path>` — create/open the database.
- `discover --root <path> --db <path>` — discover supported candidates; does not collect.
- `sources --db <path>` — list candidates, IDs, status, and timestamps.
- `connect <source-id> --db <path>` / `decline <source-id> --db <path>` — persist consent choice.
- `connections --db <path>` — list connection state and collection status.
- `pause|resume|disconnect <connection-id> --db <path>` — control future collection; disconnect does not delete evidence.
- `collect --db <path>` — run approved enabled connections.
- `events --db <path> [--raw]` — combined factual timeline.
- `event <event-id> --db <path> [--raw]` — inspect one factual record.
- `import git --repo <path> --db <path>` — legacy direct local proof command.

Unknown IDs, duplicate approval, and invalid state transitions return nonzero errors.
