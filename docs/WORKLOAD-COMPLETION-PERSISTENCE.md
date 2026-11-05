# Asynchronous workload completion persistence

The existing internal issuer now awaits its journal read and completion save
without running SQLite on the async caller's thread. This uses the same `JobStore`
and the reporting slot reserved before issuance; it adds no scheduler, new queue
capacity, source, execution command or public transport.

## Close authority before saving history

The active caller's original observation deadline is capped by the issuer's
original monotonic deadline. Inspection uses the bounded native reader, with
local clock/reporting health and exact retained-definition checks. Cancelling
that read retires the kept token and queues its reserved uncertainty checkpoint.

Completion still requires the original pinned peer identities, exact definition,
durable consumed ACK, valid typed completion report and a sequence the original
window actually issued. Invalid reports do not retire a healthy issuer. After
validation, the original window closes and the token becomes permanently retired
before its reporting slot moves to the native completion job. There is no await
between closing the window and submitting that historical work.

The worker receives the bounded definition and completion metadata, exact-store
reporting slot and journal ownership. It receives no issuer, lease, source future
or runtime handle. The existing strict job edit revalidates the consumed ACK,
definition, allowed state and report before saving the schema-three terminal fact.
Existing byte quotas, payload limits, event evidence and lease ceilings remain.

## Cancellation, late saves and failure

Once admitted, historical persistence does not disappear when its caller stops
waiting. Its reserved capacity and journal ownership remain charged to the actual
operation. Observation can time out while that save finishes later; a late saved
completion cannot renew a lease or replay a source. After actual success, an
identical report can read the retained terminal history under a fresh read-only
observation deadline, including after the old issuer expires. Changed reports,
unknown tracking or a different peer cannot use that path to refine history.

If the completion write fails or panics, that same worker attempts exactly one
strict uncertainty checkpoint before releasing its original slot. Dropping or
retaining the token never retries completion or queues a second fallback. Failed
fallback or unavailable queue admission latches reporting denial on that store
handle; removing a fault does not clear it. The earlier record remains evidence
of uncertainty, and restart cannot reconstruct an issuer from it.

The idle queue owns no journal. Runtime destruction does not join a blocked native
save, and replacement waits for actual connection/file-lock ownership to end.
Completed callbacks release their slot and journal before waking the observer.
Retirement and historical node reports do not independently prove physical effects
have stopped.

## Verification and remaining work

Four new shared regressions use actual temporary SQLite journals and synthetic
node reports: cancellation while retaining the token and reporting reservation;
an observation deadline with a later durable completion; failed completion and
failed fallback with no retry or authority restoration; and runtime destruction
while a native save retains journal ownership. Existing completion, corruption,
receipt-inspection and retirement cases now await the same method with their
assertions retained. No node, source, model, interpreter or hardware runs.

Fresh hosted compilation and complete acceptance are required. Initial issuance
and other legacy coordinator operations still need broader asynchronous work.
Authenticated public continuous execution, owner command integration, long source
sessions, physical two-PC validation and stable release acceptance remain open.
