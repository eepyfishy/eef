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
result and stop-inspection checks remain required. The foundation's checks are
included in the accepted source matrices recorded in [status](STATUS.md).
The basic-job extension below needs its own verification.

## Basic public job inspection (development)

Basic job inspection now shares this existing four-slot reader. The owner HTTP
handlers for `GET /api/jobs` and `GET /api/jobs/{id}`, and authenticated origin
commands `jobs.list`, `jobs.find`, `jobs.get` and `jobs.output`, use the same
historical service with a five-second caller deadline. It admits only those
read operations; creation, removal, pause, resume and Stop cannot enter this
callback. There is no synchronous fallback when the reader is busy or late.

The existing owner/origin filtering, output selection, redaction, validation and
JSON responses remain unchanged. An origin-looking payload field cannot replace
the context supplied by the authenticated submission handler. This adds no
principal registry, information-flow policy or module isolation guarantee.
Output inspection remains explicit; moving it to a worker does not authorize a
new recipient or a control operation.

The actual read retains capacity and journal ownership after caller cancellation.
Its result cannot acquire a new deadline or be presented after the original
deadline. Other consumers of this reader share its existing capacity; no new
thread pool, store, cache or execution authority is introduced. The direct Rust
`job_request` interface remains synchronous for existing internal callers;
mutating public job operations still need their own integration.

New regressions cover the actual public handlers under journal contention,
cancelled-reader ownership, unchanged origin filtering and explicit text output,
and rejection of controls before worker admission with zero journal writes.
Initial output fixtures were rejected by the existing fresh-job and task-origin
rules. The corrected setup supplies origin to the planner, admits a pending job
and checkpoints synthetic completion through the ordinary journal edit. All three
focused regressions pass. Frozen source `65cae5e` also passes the full local
workspace/all-target run: 1,951 tests with three ignored across 20 groups.
[All four hosted jobs](https://github.com/eepyfishy/eef/actions/runs/45921897326)
also pass. Raw logs confirm 1,980 Windows/headless Rust tests with three ignored,
all 40 native suites and retained extra gates. Windows/dashboard passes 1,951;
both Linux jobs pass 1,879. Matching-package acceptance remains pending; deployed
packages have not been updated.

This is the read side of the coordinator journal integration. Issuance,
consumption ACKs, event/completion writes, `AttemptJournal` updates and remaining
legacy synchronous callers have independent worker contracts and acceptance
gates. This read path does not establish general journal responsiveness or
public continuous execution acceptance. No new public launch command,
continuous-source time, model limit, permission or stable-release claim follows.

## Legacy owner task history (development)

The owner-only `GET /api/tasks` handler now uses the same four-slot historical
reader and five-second observation deadline. Its existing `plans` response and
complete retained-plan serialization are unchanged. This does not create a node
submission command or widen the route's existing access boundary. Basic job
views retain their separate redaction and explicit output-selection rules.

The extended public-handler contention regression includes this task-list
handler alongside job get/list. It checks current-thread timer progress, shared
admission, capacity retention after caller cancellation, unchanged responses and
zero journal writes. Frozen source `9604ced` passes the complete unfiltered
workspace/all-target matrix in [all four hosted jobs](https://github.com/eepyfishy/eef/actions/runs/46109575905):
Windows/headless has 1,995 passing Rust tests, three ignored and all 41 native
suites; Windows/dashboard has 1,966 passing tests and both Linux jobs have 1,894.
The public-handler test is compiled in each configuration; quiet test output
reports group outcomes rather than individual test names.

This follow-up is separate from the frozen alpha.9 installation candidate at
`c179ce2`. Matching packages and physical acceptance remain pending for this
task-history change. Pause/removal integration is described below; creation,
resume and remaining internal synchronous callers still require separate work.

## Pause and removal (development)

Owner HTTP and authenticated origin-node pause/removal requests now use a
separate two-slot native history-control worker. It reuses the existing bounded
queue implementation and original journal. It cannot consume the four reader
slots or Stop's separately reserved admission. Full admission refuses without a
queued retry or a synchronous fallback. Only pause/removal can enter this
service; creation, resume and Stop retain their separate execution/control paths.

Each caller has one five-second observation deadline. The worker retains its
actual slot and original journal until its native operation ends, even if the
async caller is cancelled or times out. Origin matching and caller currency are
checked while holding that journal, before changing the retained job. A queued command
whose caller has gone or whose original deadline expired is discarded. Removal
still checks finished state and schedule/occurrence lineage; reviewed execution
still refuses pause without an acknowledgement contract. Existing view redaction
and response JSON shapes are preserved. Synchronous Rust callers keep their
original interface.

Cancellation does not undo a write already entered. A native write can finish
after its caller's observation ends; inspect the job before issuing another
command. This adds no execution dispatch, principal verification, stronger remote
stop or OS isolation guarantee.

Three new focused regressions cover origin/state rules, blocked-journal timer
progress, caller cancellation, expired queued deletion, retained capacity,
independent Stop/read admission, and rejection of execution operations. The
public-handler contention regression also exercises pause/removal alongside
historical reads, cancels those controls before releasing the journal, and checks
unchanged job state with zero writes.

Frozen source `3877f46` passes [all four hosted configurations](https://github.com/eepyfishy/eef/actions/runs/46167808003).
Windows/headless passes 1,998 Rust tests and all 41 native suites;
Windows/dashboard passes 1,969 and both Linux configurations pass 1,897, each
with three ignored tests across 20 groups. The first source failed compilation
because an existing ignored timer-profiling fixture omitted the new queue field.
The corrected source adds that field without removing a test or weakening a gate;
the failed run remains retained separately.

Matching packages and physical acceptance are pending. This increment is not in
the published alpha.9 installers. Creation, resume and other synchronous callers
still require their own integration; this is not general journal responsiveness.
