# Observing workload cancellation ordering

The shared-interpreter-admission source `eeb6aed` passed both Linux jobs and
Windows dashboard in its
[hosted run](https://github.com/eepyfishy/eef/actions/runs/42732933482). Windows
headless failed the existing cancelled-event-waiter test: after dropping the
reply receiver, available write slots were four rather than the expected three.
The terminal log is retained. This run remains failed; it cannot accept that
development source as a whole.

The event worker checks for a closed observer before entering the journal and
again inside the guarded edit. The test held the journal but did not establish
that the worker had passed its first check before cancelling. A queued worker
may observe cancellation immediately and finish without ever waiting for the
journal; releasing its slot at actual completion is valid. Holding the journal
alone does not establish an in-progress writer. This source-level ordering gap
explains why the old assertion was too strong; the log does not identify the
precise scheduling order on that runner.

The follow-up adds test-only entry counters immediately after the event and
stop workers' early checks. Cancellation while waiting for a held journal now
waits for that observed entry, instead of assuming native-thread scheduling or
sleeping for 20 milliseconds. It still requires capacity to remain occupied
after cancellation and until the actual worker exits, with no late history edit.
Original worker deadlines and existing positive observation windows are retained.

A separate queued-event test holds the existing serial write worker using a
test-only barrier, cancels the queued event, then releases that worker. It
requires capacity to return, no guarded entry, and byte-equivalent saved state.
The two orders have distinct observations and the same no-late-write rule.

All added worker fields, counters, accessors and the barrier are `cfg(test)`.
Production code paths, queue capacity, journal locks, cancellation checks,
deadlines, leases and permissions are unchanged. No extra production worker,
poll loop, retry or dependency is added. Hosted acceptance is pending. This
test synchronization does not prove physical storage performance, strong stop,
automatic input integration or stable-release readiness.
