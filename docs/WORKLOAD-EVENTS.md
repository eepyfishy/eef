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

One owned blocking event worker per ledger handle performs SQLite work. It retains
its capacity permit even if its async waiter is cancelled while a started write
is still blocked. A second worker is refused; there is no unbounded spawn queue,
offline event buffer or automatic retry. The prepared transport awaits one save
and observer at a time, within the reviewed pending-event limit.

The waiter and worker share the same captured reporting checkpoint. Checks before
and after lock/transaction/commit bind the consumed contract, live supervisor and
original conservative deadline; valid renewals cannot rejuvenate an older save.
Stop intent is checked both in memory and in the existing durable stop table.
Reporting-control locks are never held across journal I/O. Source cancellation
retires the source future before the existing final uncertainty checkpoint. An
event committed while a waiter expires is historical evidence only: its late
result cannot deliver an acknowledgement, renew a lease, restart or replay work.

The existing final uncertainty save is still synchronous. A stalled final save
can delay its task after the source has been retired; making final reporting
asynchronous and bounded remains required for responsive public admission.
Likewise, a transport summary or saved event prefix does not attest completion.
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
This journal draft requires hosted verification; local checks run formatting,
source guards and a tiny in-memory SQL catalog check only.
