# Bounded coordinator evidence reads

The existing `requests receipt`, `requests result`, and `requests stop-receipt`
services now read their coordinator job through a bounded native journal worker.
This applies to the local eef owner and authenticated origin-node commands;
workload receipts use the same path. A stalled coordinator SQLite mutex no longer
runs directly on their asynchronous runtime thread.

The same `JobStore`, connection, validation, quota and exclusive file ownership
are used. This is not a new job database or request scheduler. One lazy worker
per store admits at most four reads in total, including the executing read.
Capacity stays held by the actual operation after its waiter times out or is
cancelled. A full queue refuses instead of waiting, growing, retrying or falling
back to synchronous execution. Queued reads whose waiter has gone or original
deadline has expired are discarded before querying.

Each evidence command captures one ten-second deadline before the coordinator
read. Its subsequent independently authenticated executor query shares that
deadline rather than gaining another allowance after storage delays. A result
observed after the deadline is refused, including an immediately ready late
response. Reads retain the existing origin, exact issuance and typed evidence
checks; they never reconcile coordinator state, start a source, renew a lease,
send stop intent, or repair a record. Evidence can become stale after inspection.

The idle worker owns no store or runtime. An admitted reader keeps the exact
journal alive until its actual read ends; cancelling its waiter does not unlock
or replace the database. Runtime shutdown does not join a blocked SQLite read.
The existing journal-retirement signal still waits for the connection and file
lock to be released. A query already entered can outlive its caller; the bound
limits observation, not an OS filesystem operation's duration.

Four new regressions use actual temporary journals and an explicitly held
coordinator mutex. They cover unchanged and damaged history, bounded capacity
after cancellation/timeouts with current-thread timers still progressing,
expired-before-admission refusal, idle-store retirement, and runtime shutdown
while a reader holds journal ownership. A fifth regression refuses executor
inspection before polling an expired future and rejects an immediately ready
response that crosses its original bound. Existing owner/origin/TLS receipt,
result and stop-inspection checks remain required. Fresh hosted validation is
pending; no heavy local build or runtime test was started.

This is the read side of the coordinator journal integration. Issuance,
consumption ACKs, event/completion writes, `AttemptJournal` updates and other
legacy synchronous reads still require broader asynchronous integration before
public continuous execution can be accepted. No new public launch command,
continuous-source time, model limit, permission or stable-release claim follows.
