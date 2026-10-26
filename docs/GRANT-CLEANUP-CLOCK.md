# Deterministic grant-cleanup ordering

The cleanup-order fixture must distinguish in-flight cancellation from refusal
of work whose lease already expired during setup. The earlier fixture required
an operation's first poll after starting a one-second deadline before durable
claim I/O and thread/runtime startup. Hosted `4a9a24f` failed that assertion;
which exact stage consumed its time was not recorded. A later green run does
not establish that cause or erase the failure.

A private execution-clock seam keeps the normal `run_operation` entry point
bound to `SystemClock`. Admission, authenticated lease creation, paired clocks
before durable claim, stored deadlines and wire/configuration formats do not
change. The normal implementation uses the same wall-clock read, monotonic
elapsed calculation, absolute Tokio deadline, drift threshold, select priority,
stop/policy checks and completion-after-deadline downgrade. No public or runtime
configuration can select the synthetic clock, renew a lease or raise a budget.

Only synthetic cleanup-order tests use controlled monotonic and paired wall
time. They first require the actual operation to poll, then explicitly advance
to the existing one-second deadline. Completion, failure and permission-loss
cases retain their distinct triggers. A wall-only jump separately exercises
clock-drift refusal; a notification-before-sleep test checks the wake path.
This tests ownership ordering, not real elapsed time or durable-I/O latency.

The initial unpublished draft added an extra async wrapper around execution.
Both Rust modes passed, but the real logical-node audio fixture failed before
acknowledgement: its destination exited with Windows `STATUS_STACK_OVERFLOW`.
No source grant was consumed or source data dispatched. A layout regression
then proved that, for a suspended synthetic 64-KiB operation, the normal wrapper
future occupied 198,768 bytes versus 132,624 bytes directly. Those figures are
for that representative future, not a measurement of the peer's exact stack.
The precise crash call site was not captured.

The normal entry point now returns the implementation future directly, avoiding
another stored async operation frame. Callers still await the same operation;
no larger stack, new heap wrapper or wider deadline is used. The layout test
compares actual executor future types without constructing a grant/engine,
polling an operation or allocating its large synthetic buffer at runtime.
The failing native fixture and unequal-layout result remain failed evidence;
fresh corrected-layout and real pipeline validation are required.

The actual journal mutex remains held. Original assertions remain: the operation
was polled, its guard retires before final storage can proceed, the worker has
not returned, the stored receipt is still accepted, the final confirmed report
persists, private error text is absent and the same grant cannot replay.
The existing real native root/descendant test still uses the normal real-clock
entry point and held process handles; no synthetic clock replaces it.

A separate real-clock case only narrows a synthetic lease to an already-expired
deadline. It requires that the operation never polls, its captured guard still
retires before blocked final storage, the result stays effects-unknown with a
deadline code, the receipt persists after release and replay is denied.
Expired-before-start refusal is correct behavior, not a reason to rejuvenate a
lease or suppress the in-flight test's first-poll assertion.

Focused, full-workspace, native and exact-revision hosted acceptance remain
separate. This change does not claim remote physical effects stopped, model
quality/default acceptance, installer rollback, signing/Defender clearance,
physical two-PC acceptance or stable v0.4.0 completion.
