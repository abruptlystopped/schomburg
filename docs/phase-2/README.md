# Phase 2: Presentation

The Record Folder is a generated, disposable read model from SQLite evidence. It is not the source of truth and can be deleted and regenerated. Generation is Git-only today, uses connector-owned factual presentation, and does not summarize or infer.

Phase 2.4 adds `schomburg-service`, a portable, structured control API shared
by the CLI and future native shells. It coordinates the agent, store,
connectors, presenter, and operational configuration without owning evidence.
The CLI is now another service client; native shells must call the service
rather than parse terminal output. macOS is the first planned shell, not the
owner of Schomburg, and Windows will use the same boundary.

Update Record is protected from overlapping calls within one service instance.
Platform-specific folder opening, cross-process locking, and the long-running
scheduler lifecycle remain deferred.
