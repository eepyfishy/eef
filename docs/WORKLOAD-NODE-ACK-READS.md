# Node ACK inspection before source execution

The prepared microphone runner and its consumed-source supervisor inspect their
saved node ACK asynchronously. An original consumption and lease proof remain
on the caller. A native reader receives only the exact journal and consumed scope;
it cannot create a proof, source, renewal or new consumption.

Each observation is bounded by both its caller deadline and the original initial
lease deadline. The read shares the existing single event-worker permit and lazy
workload journal worker. Actual queued/started work retains that permit and journal
until it finishes, including after cancellation or async runtime destruction.
No production thread pool, work queue capacity, lease, storage or permission limit
is increased. The reader releases its own capacity/store reference before replying.

The caller checks original proof liveness, reporting health and stop intent before
and after the read. The returned ACK must still be accepted and match the exact
consumed definition/scope. A stop arriving after a successful native read still
refuses source startup. A late read can return historical data but cannot refresh
the original proof or start a source after its deadline.

Cancellation or failure permanently removes the caller's initial proof and retires
reporting in memory. It keeps the original final reporting reservation on the
consumption. The existing runner cleanup drops its owned source future first,
then consumption cleanup submits uncertainty using that original reservation.
This ordering also applies while an ACK read is blocked. A caller that keeps a
retired consumption must explicitly retire or drop it to finish that cleanup;
it cannot re-use the saved ACK or restart the same consumption.

The prepared runner builds its event writer from the checked ACK metadata instead
of performing another synchronous journal read. This does not create reporting
authority: event appends still require the original bound reporting checkpoint,
exact node journal, lease and stop checks. The original runner performs its own
bounded ACK check before first polling the source. Synchronous receipt inspection
is retained for native admission and existing explicit inspection/test callers.

Six new real-journal regressions cover:

- Cancelling a read while keeping the consumption, without releasing actual read
  capacity or making the old ACK usable again.
- Both caller and original initial-lease deadlines during a blocked read.
- Stop intent recorded after a native snapshot but before caller confirmation.
- Unreadable retained definition bytes, with no evidence repair or source start.
- Dropping an unpolled owned source while the ACK reader remains blocked.
- Runtime destruction while a real SQLite read and finalization retain the exact
  journal, without keeping a source future alive.

Only the last contention fixture extends its temporary SQLite busy wait to three
seconds to observe native ownership during teardown. The production busy timeout
remains unchanged. Existing source, event, completion, lease, reporting and native
Windows process/stack regressions remain required. No heavy local build or runtime
test is performed; changed-source hosted validation and final main-ref acceptance
must pass before acceptance is claimed.

This removes blocking ACK reads in the existing prepared source path. Native
consumption admission itself remains synchronous; the complete authenticated
offer/probe/consumed-ACK/events/completion/stop route and owner execution commands
still need integration and acceptance. Source execution is not newly advertised.
Physical two-PC, long-source and stable v0.4.0 acceptance remain open.
