# Node consumption before source startup

Consumption persistence now uses the existing node journal worker asynchronously.
The authenticated caller must first check the current source, permissions, consent,
runtime and resource reservation. This persistence API creates none of those.
The original initial lease proof remains on the caller throughout the await.
There is no proof, source, source factory or runtime in the native write job.

The caller supplies a monotonic deadline. Admission clamps it to the original
initial proof deadline without resetting either clock. It reserves the original
final reporting slot before SQLite and shares the existing single event-worker
permit and bounded native queue. Busy capacity refuses immediately; there is no
automatic retry, additional worker pool or increased production storage/lease
limit. Actual native work retains its capacity and exact journal across caller
cancellation or runtime destruction.

Inside the immediate transaction, admission retains the existing stop/attempt
checks, owner validation, schema migration, quota reservation, exact definition
insertion and event-journal initialization. It checks the original fixed deadline
and waiter liveness after acquiring storage and before mutations/commit. Failed
transactions roll back the schema and insertion together. The existing production
SQLite busy timeout is unchanged.

A successful metadata packet owns the original final reporting slot and exact
stop watcher. It is private, non-serializable and non-cloneable. Losing this packet,
including after a committed save but before caller confirmation, delegates one
uncertainty checkpoint with that original slot. Saved bytes cannot reconstruct a
lease or restart authority. Failed writes inspect only possible retained evidence
on the native worker: absent consumption is uncommitted, exact retained active
consumption becomes `unknown`, and unreadable evidence denies further admission
without repair. Valid unrelated or terminal evidence is preserved.

The worker releases its own write capacity and journal reference before waking
the caller. The caller revalidates the original proof/deadline and transfers its
original reservation into the consumption, then awaits the existing bounded ACK
inspection. Stop intent, reporting failure, changed evidence, a lost read or an
expired proof prevents admission from returning authority. Existing source-first
cleanup ordering still applies once the prepared runner owns the consumption.
An accepted receipt describes durable consumption; it is not source success.

Eight new real-SQLite regressions cover cancellation during a held connection
mutex, both caller and proof deadlines, loss of committed metadata while final
storage is blocked, post-commit proof expiry, post-commit stop, runtime destruction
with actual native journal ownership, an aborted insertion with migration rollback,
and damaged post-commit evidence. Existing tests retain attempt uniqueness across
independent handles, owner/quota/stop checks, source retirement and completion.
The ACK reader fixtures now account for the first read performed by admission.
No source, microphone, model or installed node is launched by the new cases.

Initial run `42132439721` caught the older SQLite lock-wait case asserting at
least one second elapsed on the caller. Its original one-second issuer horizon
already produces a shorter proof through the existing conservative margin.
The synchronous API used to wait for later native lock release; the async API
correctly ends its wait at the captured original proof deadline. The fixture now
asserts that exact unchanged deadline, then waits for actual native rollback before
checking schema version and zero consumed rows. No production deadline, margin,
busy wait, rollback assertion or source authorization rule is relaxed.

Formatting and source checks run locally; compilation/runtime verification runs
on GitHub. Full changed-source hosted and final main acceptance are required.
The authenticated node dispatcher, owner start/stop route, physical two-PC and
long-source checks remain open. Public workload execution is not advertised by
this change, and stable v0.4.0 is not yet accepted.
