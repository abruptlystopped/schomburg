# Shell Bridge

`schomburg-host --db <path>` retains one `SchomburgService` and accepts one
newline-delimited JSON request per stdin line. It writes exactly one JSON
response per line to stdout and reserves stderr for diagnostics. The protocol
is versioned with `protocol_version: 1`; it opens no network port.

The host exposes explicit commands for status, Update Record, scheduler,
discovery, consent, connections, monitoring, Record Folder, schedule, and
shutdown. macOS and Windows shells can share this transport. The host owns no
evidence and does not parse terminal output. Direct FFI may be reconsidered
later without changing service semantics.
