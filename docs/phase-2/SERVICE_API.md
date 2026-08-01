# Portable Service API

`schomburg-service` is the shared, portable control API for the command-line
client, the future macOS menu-bar shell, the future Windows tray shell, and
later shells. `SchomburgService::open(database_path)` assembles the existing
store, agent, supported connector extensions, and presenter registry so a
shell does not assemble those internal components itself.

The service exposes structured status and operation results for discovery,
source consent, connection state, monitoring, Record Folder and schedule
configuration, and Update Record. It coordinates existing components; it does
not own Events or Annotations, interpret evidence, or parse source-specific Git
payloads. Native shells must use these structured values rather than parsing
CLI output.

Service status exposes Manual Update and Scheduled Reconciliation status
independently. Shells must not infer daily reconciliation from a manual update.

The service prevents two Update Record operations from overlapping within one
service instance. It intentionally does not yet provide cross-process locking,
a platform-native UI, or platform-specific folder opening. The portable
scheduler is available through structured start, stop, and status operations;
it remains hosted by a shell or development CLI. Cross-process locking remains
outside this boundary.
