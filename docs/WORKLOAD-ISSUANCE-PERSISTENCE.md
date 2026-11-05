# Workload issuance persistence

Internal workload issuance is asynchronous. It still sends no network offer,
opens no source, and does not make continuous execution publicly available.
Product callers must hold the engine admission/quiescence gate and verify fresh
authenticated node/resource metadata before using the internal API.

The caller supplies its original observation deadline. Before journal access,
issuance validates the command/session and reserves completion reporting capacity
from the existing owner quota. The initial read uses the bounded native reader.
Policy, exact approval, revision, request context and source constraints are
checked before a mutation is admitted.

The original caller captures the monotonic and wall anchors and constructs one
issuer window. That window stays on the caller throughout persistence and a fresh
post-commit read. It is never serialized, reconstructed from a row, or transferred
to a native worker. The caller cannot receive an issuer after its observation or
original session deadline, a clock discontinuity, reporting failure, or a changed
checkpoint.

Issuance and consumption ACKs share the existing bounded native write queue:
at most the minimum of four, the owner's job count, and its byte quota divided by
the existing 16 KiB completion reserve. No new write queue or storage allowance
is added. Retirement/completion retain their separately reserved reporting queue.
The actual native operation retains its write permit and exact journal even if
its observer disconnects or its async runtime is destroyed.

The writer receives metadata, the fixed issuance timestamp, observation deadline
and original reporting slot. It checks abandonment before journal acquisition
and again inside the edit, then revalidates exact approval/source/revision. It
cannot issue a grant, renew a lease, or start work. One successful commit produces
a private historical packet owning only the exact store, definition and reporting
slot. Losing a delivered packet, cancelling during the post-commit read, or
missing the caller deadline drops that packet and queues one strict `Unknown`
checkpoint. A saved definition alone never grants execution authority.

A failed write is inspected on the same native worker only if an edit may have
started. An exact active issuance needs one uncertainty attempt. A validated
unissued row or different retained assignment is preserved. Unreadable evidence,
a panic, or failed uncertainty denies new workload reporting/authority until
explicit owner recovery. There is no automatic write retry, evidence repair,
grant replay, replacement token or renewed consent. A clearly uncommitted
refusal preserves the approved job without falsely reporting global corruption.

Eight added coordinator regressions use real SQLite journals and contention:

- Cancellation during the initial read preserves an unissued approval.
- Cancellation inside a real write retains capacity through a late commit and
  one uncertainty checkpoint; issuance does not count as consumption ACKs.
- A lost delivered packet and cancellation during a blocked post-commit read both
  retire the committed row without returning an issuer.
- Both the original observation and session deadlines refuse late authority.
- A definitely uncommitted failure preserves approval without automatic retry.
- An unreadable uncertain write preserves bytes and latches reporting denial.
- Failed orphan retirement denies new issuance without repeated writes.
- Runtime destruction retains the real journal/file lock until native issuance
  and orphan retirement finish.

Existing approval, quota, restart, lease, ACK, completion and immutable-history
cases use awaited issuance with explicit deadlines. The manual-runtime retirement
case creates its runtime before awaiting its fixture. No production timeouts,
lease limits, permissions, schema, quota or automatic-update policy are widened.

These are internal persistence regressions. They do not establish authenticated
public continuous execution, long-running physical source behavior, two-PC
acceptance, installer acceptance, or a stable v0.4.0 release. No heavy local build
or runtime test is part of this change; complete changed-source hosted validation
and final main-ref validation remain required.

Initial hosted run `42082338014` failed compilation because one migrated issuance
call in the manual-runtime completion case still used `.await` outside an async
block. The correction uses that case's existing observer runtime. It changes no
shipping behavior, assertions, deadlines or test selection. The failed run remains
evidence; a new exact source needs complete hosted acceptance.
