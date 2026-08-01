# ADR 0012: Portable shell-facing service boundary

## Decision

Introduce `schomburg-service` as the structured portable control API for the
CLI and future native shells. The service opens the local store, registers
supported connectors and presenters, and coordinates the existing agent,
presenter, and operational configuration. It exposes source discovery and
consent, connection state, monitoring and schedule configuration, Update
Record, and structured status.

## Consequences

The CLI is a service client rather than an assembler of user-level internal
components. A macOS menu-bar shell is the first planned shell, not the owner of
Schomburg; a Windows tray shell will use the same service API. The service does
not own evidence, duplicate engine/store behavior, or parse Git payloads.
Shells must consume structured status rather than terminal output.

An in-process guard prevents overlapping Update Record calls per service
instance. Cross-process locking, scheduler lifecycle, and platform-specific
folder opening are deferred.
