# Durable workload event journal draft

The internal prepared microphone runner now saves each useful metadata event in
the existing `GrantLedger` database before constructing its observer callback.
No startup, public command or network route launches a source through this draft.
There is no second job database, automatic replay or completed-workload receipt.

## Evidence and bounds

Event schema two adds elapsed milliseconds from the first received source
observation, which is zero for the first event. These are relative observation
times, not wall-clock timestamps or authority to resume. The existing typed
microphone activity, separate event/source sequences, exact source frame positions,
unknown/gap semantics and approved rate/count/byte limits remain. The first event
must describe the first source observation. Later records must preserve source
order, event order, minimum interval and a real state change or explicit gap.
The new elapsed field counts against the same canonical encoded-event byte budget.
No PCM, transcript, private device selector, arbitrary plugin or path is stored.

Records retain their conservative lease-window sequence and local recording time,
canonical payload checksum and a prefix checksum bound to the exact consumed
definition. Exact duplicate events return the original evidence without changing
timestamps/counters or renewing authority; conflicting duplicates and skipped
sequences refuse. Normal appends inspect the tail instead of rewriting a whole
event array. Duplicate inspection and recovery reads validate the complete prefix.
Stored payload, checksum and schema text sizes are checked in SQL before Rust
materializes them. Inspection pages are bounded to 128 records and expose no
execution, restart or source-termination authority.

## Existing journal and storage accounting

The first successful consumption using this draft upgrades journal version two
or three to version four in the same immediate transaction as consumption. Two
indexed sidecar tables inside the existing database hold event state and rows.
Legacy operation results and workload definitions are validated and retained;
version-three workloads acquire empty event state, never fabricated observations.
Version-two ordinary operation use stays unchanged until workload consumption.
Old binaries refuse version four. Missing, incomplete, orphaned, corrupted or
downgraded event evidence is retained without repair or automatic migration.

Admission reserves conservative row, index, page-split and counter overhead in
addition to the existing event-payload/result reservation. Outstanding accepted
workloads and ordinary operations share that accounting. Reservations do not
allocate disk space; actual retained SQLite pages and the existing maximum page
count still apply. The owner's existing journal/result limits are unchanged.
A tight allowance can refuse a workload before consumption or stop a source on
save failure. Retired logical reservations do not erase retained events or pages.

## Worker and source lifetime

One lazy native journal thread per ledger handle performs event and final-status
SQLite work. Its bounded queue owns metadata only, never a source, process or
renewal handle. One event permit stays charged until its actual save ends, even
if its async waiter is cancelled while storage is blocked. A second event save
is refused; there is no unbounded spawn queue, offline event buffer or automatic
retry. The prepared transport awaits one save and observer at a time, within the
reviewed pending-event limit. Runtime shutdown does not join this journal thread.

The waiter and worker share the same captured reporting checkpoint. Checks before
and after lock/transaction/commit bind the consumed contract, live supervisor and
original conservative deadline; valid renewals cannot rejuvenate an older save.
Stop intent is checked both in memory and in the existing durable stop table.
Reporting-control locks are never held across journal I/O. Source cancellation
retires the source future before the existing final uncertainty checkpoint. An
event committed while a waiter expires is historical evidence only: its late
result cannot deliver an acknowledgement, renew a lease, restart or replay work.

Final uncertainty saves use the same worker. Admission reserves one final-status
slot before consuming a workload, retains it for that workload's lifetime, and
transfers it to the save until the actual storage operation ends. A live workload
or pending final save refuses a second workload on the same handle before waiting
on its primary database mutex. Independent handles still share the durable
attempt, quota and SQLite transaction fences. This is an internal single-source
admission bound, not configurable multi-source scheduling or a global lock.

Normal return drops the source and retires reporting before awaiting the final
save for at most three seconds. An otherwise successful source returns an error
if the save cannot be confirmed within that wait; an original source error stays
an error. Cancelling the whole task or dropping an unused consumption queues the
same save without waiting for storage, including after its Tokio runtime is gone.
Dropping or timing out the waiter cannot free capacity before the real save ends.
The wait is after source retirement and does not extend execution authority.

A failed final write, worker submission or caught final-save panic denies new
workloads on that handle. The retained accepted row remains consumed and uncertain;
there is no automatic retry, replay, journal repair or claim of completion. A
timeout alone does not assert storage failure: a late successful save may record
uncertainty and release its slot. A process exit can leave accepted evidence; a
reopened journal still validates retained state and never reconstructs execution.

A transport summary or saved event prefix does not attest completion.
Typed observed source exit, durable completion, authenticated public offer/lease/
event/stop wiring, long-session transport and physical acceptance remain open.

## Verification boundary

Eleven added regressions use actual temporary SQLite journals and synthetic
consumed operations: read/reopen/pagination, idempotent/conflicting duplicates,
strict metadata/rate/count/byte bounds, shared storage reservation, stop/retirement,
failed insert rollback, corrupt/oversized/orphaned/downgraded evidence, version-three
migration, cancelled-worker capacity retention and original-lease expiry during
a real SQLite write-lock wait. The existing opt-in combined Python/native-source
test also requires each callback's exact event to be readable durably first.
The event-journal checkpoint passed hosted run `41139146907`, including all four
jobs, 1,352 Windows headless Rust tests plus one existing ignored test, and all
36 native suites with the extra listener-close case. Six added final-status
regressions cover blocked final saves, waiter cancellation, dropping without a
runtime, failure denial, reserved capacity and post-consumption stop-registration
failure. Existing cancellation tests now also require the runtime to exit while
storage is still blocked. These asynchronous-finalization changes require their
own hosted verification; local checks run formatting and source guards only.
