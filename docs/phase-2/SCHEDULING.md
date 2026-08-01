# Scheduling

Phase 2.3A stores operational configuration, not evidence: monitoring state, Record Folder path, Daily/Weekdays/selected-weekday schedule, local time, next eligible run, and status fields. Monitoring enabled means eligible for future scheduled reconciliation, not constant surveillance.

Scheduling uses the machine local timezone; changing timezone can change the calculated local next run. The Record Folder path is separate from Events. No scheduler process or automatic reconciliation runs in 2.3A. Phase 2.3B adds reusable `reconcile_once`; Phase 2.3C adds the long-running scheduler lifecycle.
