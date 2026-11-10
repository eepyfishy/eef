# Renewal retirement and historical completion

This unreleased follow-up awaits hosted validation. It changes neither lease
budgets nor source/journal authorization and adds no dependency or worker.

The live workload supervisor owns the renewal receiver. It drops that receiver
after the source finishes or fails, before the bounded final journal save returns.
The transport client previously ran renewal against the entire source future,
including that historical save. Sending a proof to the now-closed receiver could
report an ended runner and drop the completion waiter even though live source
work had already ended. The existing worker still owned any actual queued save;
an unconfirmed transport response was not proof that the save failed.

Renewal now observes closure of that exact receiver, including while sleeping
or awaiting a correlated probe reply. It cancels the pending request and returns
a private lifetime marker. That marker is not a lease, execution authority or
completion receipt. The client then awaits the same source future's bounded
historical result, without restarting source work or requesting another renewal.
Only the actual validated journal result can produce a completion response.

When the receiver remains live, expiry, failed challenges and other renewal
errors still cancel the source. Session stop, owner policy and runtime controls
remain in place. Historical-save failure remains an error; cancellation does
not refund capacity held by actual native persistence or restore reporting.

Three new coordination regressions use real encrypted in-memory framing and
correlated probe requests. They hold an explicit simulated final-save barrier,
retire the receiver while a challenge is pending, inject a late reply, and check
that the same waiter reports its actual success or failure once. Other cases
check closure after the old deadline without sending a probe, and cancellation
on expiry while the receiver is still live. These tests do not execute a camera
or substitute for the existing real-SQLite/native-source suites.

The earlier Windows/headless failure in source `990847c`, run `42816124759`,
retained five camera events, observed owned-process exit and source closure, then
returned `source_unconfirmed` with the generic `workload_unconfirmed` reason.
Those observations alone do not identify the underlying error. This change fixes
a source-lifetime race found during that investigation; it does not establish
that race as the cause of the recorded failure. Keep that failure and its logs,
require complete fresh CI, and retain physical camera/multi-PC and stable-release
gates separately.
