# Durable workload consumption

The node journal draft extends the existing `GrantLedger` and
`execution_receipts` table. It does not add another job database. Operations and
workloads share grant IDs, issuer/job/task/attempt slots, stop intent, receipt
counts and reporting reservations. Changing operation kind or increasing an
attempt number cannot bypass unresolved consumption.

The internal workload mutator requires a fresh initial lease for the exact
definition and pinned issuer/session/node. Its policy defaults disabled. It
validates the owner reporting bound and spends the original lease during journal
I/O. The same SQLite immediate transaction validates retained legacy evidence,
upgrades the schema and inserts the complete definition before acknowledgement.
A failed quota check, lock wait beyond expiry or write rolls the transaction
back. An expired lease after commit still leaves authority consumed; the owned
token records uncertainty rather than issuing an execution acknowledgement.

Ordinary operation use keeps journal version two. The first successful workload
consumption upgrades it to version three, retaining legacy rows/results and
adding a receipt kind and exact definition. Older binaries refuse version three
when opening it. Do not overlap older runtimes or downgrade a live journal.
Malformed, incomplete or downgraded schema/evidence is retained without repair.
Generic consumption metadata remains available for the existing stop machinery;
operation-output readers explicitly refuse workload rows. The typed library
reader validates the entire definition/digest and reporting reservation.

The token is neither serializable nor cloneable. It owns an existing stop
watcher. Its acknowledgement refuses an expired initial lease, changed receipt
or observed stop. Dropping it attempts one uncertainty checkpoint without retry;
it never records successful execution. A crash or failed uncertainty write may
leave an `accepted` row. That means consumed and potentially interrupted, not a
live source. Reading or reopening the journal never restores a lease or starts
work. These receipts do not prove that OS, device, backend or external effects
stopped.

The consumed-runner development path transfers the original non-cloneable initial
proof once into the existing renewal supervisor. It rechecks the retained receipt
before polling one prepared source future and observes the existing stop watcher.
It never constructs a replacement source on renewal, changes the absolute session
end or replays consumed authority. An explicit destruction guard drops that future
before final synchronous uncertainty persistence, including cancellation of the
entire task. A source result alone still cannot create a completion receipt.

The reporting lifetime follow-up binds one read-only control to the exact consumed
runner. It is inactive before supervision and permanently retires on cancellation
or return, including before blocked final persistence. No retained receipt or
cloned reporting handle can restore it. Prepared event callbacks use captured
conservative windows; later renewals cannot keep a stalled callback pending forever.
This adds no schema, durable event/completion or asynchronous journal writer yet.

Five new real-journal regressions cover stop, whole-task cancellation, retirement
before the first source poll, missing renewal and the absence of false durable
completion. Their source is a synthetic pending future whose destructor reads the
actual SQLite receipt; this verifies future/persistence ordering, not physical
process or device termination. Hosted validation is pending.

This remains an internal persistence boundary. No command, startup, registration
or authenticated network handler invokes the mutator. It does not issue a grant,
check microphone/listening permission, acquire a resource, select source bytes,
implement an event buffer or enforce physical CPU/device budgets. Reporting space
is reserved conservatively using the owner journal policy; event persistence and
delivery are still absent. There is no workload completion/output record yet.
Product admission must integrate the existing coordinator `JobStore` and
`AttemptJournal`, persist issuance before dispatch, validate consumed ACKs on the
authenticated session, and retain resource/permission/lease ownership before and
throughout actual effects. No parallel job framework or global v1 duration
increase is authorized by these primitives.

Tests use actual temporary SQLite journals and synthetic lease challenges. They
cover legacy result retention, exact-definition consumption before ACK,
cross-kind replay fencing, racing independent connections, pre-admission stops,
stop notification, policy/quota refusals, lease expiry during a write-lock wait,
rollback, owner/version mismatches and malformed records without repair. They
do not establish product admission, real capture or physical network acceptance.
