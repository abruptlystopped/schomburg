# Scheduler Lifecycle

The portable scheduler lives in `schomburg-service`. A shell starts it, stops
it, and reads its structured lifecycle status. It waits efficiently for the
persisted Daily, Weekdays, or selected-weekday local schedule, and each
scheduled run calls the same Update Record operation used by the manual action.

The underlying pipeline is shared, but scheduled runs write a separate
Scheduled Reconciliation status. A manual Update Record never closes the day,
postpones the schedule, or marks the eligible date reconciled. A scheduled
no-op run remains a successful reconciliation for its intended local date.
The agent receives an explicit scheduled run kind, so no compatibility
save/restore bridge exists between scheduled and manual status.

Monitoring enabled means that a run is eligible and scheduled; it does not mean
continuous surveillance. Paused monitoring performs no automatic update. A
configuration change wakes the scheduler so the next calculation uses the
latest schedule, Record Folder, and connection state.

Alpha missed-run policy: if the process starts after an eligible persisted run
and that run has not succeeded, it performs one prompt catch-up update. It does
not replay multiple missed days. After a failure, the scheduler records the
failure and proceeds toward the next normal eligible run. Automatic retry,
cross-process locking, startup-at-login, and native platform shells are not
implemented.
