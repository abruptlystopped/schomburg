# ADR 0013: Portable scheduler lifecycle and missed-run policy

## Decision

Place the scheduler lifecycle in `schomburg-service`, where it can reuse the
same structured Update Record operation as manual clients. It waits using a
bounded condition-variable strategy and wakes when portable operational
configuration changes. The service exposes start, stop, and structured status;
native shells host and control it rather than owning scheduling behavior.

If a persisted eligible run is overdue and has not succeeded, the first alpha
performs one prompt catch-up update. It never replays a burst of historical
missed days. Failed runs persist their failure and lead to the next normal
eligible run, without automatic retry.

Manual Update Record and scheduled reconciliation persist separate operational
status. Legacy shared status is migrated conservatively to Manual Update status;
no historic record is inferred to be a completed scheduled reconciliation.

## Consequences

Monitoring Paused suppresses automatic work. The scheduler has no native UI,
startup-at-login integration, automatic retry, or cross-process locking. Those
remain separate future decisions.
