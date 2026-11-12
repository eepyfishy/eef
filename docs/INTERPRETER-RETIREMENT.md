# Nonblocking interpreter retirement

This unreleased follow-up passes complete hosted/source acceptance at `7f1d052`
in [run 42935544215](https://github.com/eepyfishy/eef/actions/runs/42935544215):
all four jobs, 1,764 Windows/headless Rust tests, one ignored across 17 groups,
37 native suites and existing extra gates. It extends existing engine lifetimes
and journal finalization rather than adding a runner, scheduler, worker,
configuration switch or execution permission.

Previously, dropping an interpreter continuation synchronously retired its
journal record. Its generic active-job guard also called the journal on drop.
Either operation could wait for SQLite on an async runtime thread. Moving only
the first write would leave that second blocking path.

Interpretation now uses the existing in-memory workload registration, retaining
its duplicate-admission, stop and shutdown gates. Dropping it revokes its live
control and retires registration without accessing SQLite. Application rechecks
that exact registration under the engine's stop gate: a failed stop checkpoint
cannot leave the original continuation able to apply a result.

Before a new allowance/source claim, admission reserves a historical-finalization
slot from the existing owner-quota-bounded queue. A full or failed queue denies
new interpretation without charging an allowance or sending inference. Existing
historical attempts remain inspectable before new capacity lookup.

After in-memory retirement, only the original attempt ID, binding digest,
reserved reporting slot and journal enter the existing native finalizer. There
is no live continuation, source future, renewal handle or model in that queue.
The original binding and existing retirement transaction remain authoritative.
The actual checkpoint retains its reporting slot and journal after its caller
disappears; queued is not saved. An inspection may temporarily see the previous
claim until persistence finishes. This cannot restore the discarded continuation
or make an unchanged claim eligible for explicit recovery.

The coordinator scheduling reservation ends with the discarded live future.
Historical metadata does not retain a model-execution slot. Durable inference
charges and uncertain-attempt limits remain unchanged; releasing a local
reservation is neither proof of remote stop nor permission to replay inference.
This also lets normal interpretation and method preparation share a single
configured model slot without waiting for unrelated historical cleanup.

Failure to submit or save retirement latches the existing shared reporting
failure gate. Later ordinary writes or removal of a storage fault do not clear
it. No automatic repair, replay, refund, replacement result or stop receipt is
invented. The existing validated-store recovery boundary remains separate.

Four added regressions use actual temporary journals and authenticated mock-model
exchanges: held journal cleanup with continuing timers and retained reporting
capacity; full shared finalization admission; a real failed SQL retirement with
no retry or evidence rewrite; and failed stop persistence preventing application.
The existing normal-input test now requires both stages to succeed with one
model scheduling slot. Earlier tests keep their assertions and wait for the
actual asynchronous checkpoint before checking retired history.

A test-only follow-up replaces the blocking checkpoint wait in the interpreter
test suites with a notification-based async wait, retaining the three-second
bound and waiting for every actual write. The held-journal case now requires
another async branch to release storage while the wait is pending, so blocking
the current-thread runtime would fail the check. Multiple outstanding writes and
waiters have a separate regression. Source `5d90895` passes all four jobs in
[run 43240147529](https://github.com/eepyfishy/eef/actions/runs/43240147529),
including 1,766 Windows/headless Rust tests, one ignored across 17 groups,
all 37 native suites and the existing extra gates. Exact source/raw-log review
confirms the held-journal and shared-state coverage. This is a test coordination
improvement, not proof of the cause of the checkpoint timeout
in `0871daf` [run 42944697616](https://github.com/eepyfishy/eef/actions/runs/42944697616).

Other interpreter journal reads, source application and stop checkpoint paths
retain their existing behavior; this does not make every coordinator operation
nonblocking. Physical multi-PC/model/media tests, general strong stop/resource
budgets, installer/security acceptance and the full stable release remain open.
