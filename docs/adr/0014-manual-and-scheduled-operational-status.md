# ADR 0014: Separate manual and scheduled operational status

Manual Update Record means “bring my record current.” Scheduled Daily
Reconciliation means “perform the eligible late-day closeout.” They share the
same lower-level collection and presentation pipeline but persist separate
status records. Manual activity never completes a scheduled date, and a
scheduled no-op is still successful for that local date.

The shared agent operation accepts an explicit run kind. The run kind controls
status ownership directly; no status is copied or restored between the two
records. This is crash-safe within one process. Cross-process locking remains
deferred.
