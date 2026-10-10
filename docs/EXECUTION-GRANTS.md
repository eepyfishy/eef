# Execution grant boundary (unreleased after alpha.6)

This is an explicit one-shot boundary for roadmap stages 6-7, **not a completed
automatic request workflow or a stable release**. The EEF
[bounded owner approval command](COMMANDS.md#bounded-owner-request-approval-unreleased-after-alpha6)
retains `approved` without issuing a grant or starting work. A subsequent
[explicit execution command](COMMANDS.md#explicit-approved-request-execution-unreleased-after-alpha6)
durably issues fixed grants in the existing job journal and checkpoints matching
acceptance/completion before confirming them. Automatic semantic planning,
uncertainty reconciliation and physical two-PC acceptance remain required.
No public API lets a proposal or LM
directly manufacture these grants. Existing legacy task/chat and management RPCs
are unchanged and are **not** claimed to be grant-enforced.

## Authority and scope

The distinct `execute_grant` wire path requires an independent exact-node
credential inside the existing owner-certificate-pinned TLS 1.3 transport. Legacy
shared-secret sessions, missing journals, unsupported older nodes and stale
connections cannot use it. There is no fallback to ordinary RPC or another node.
The authenticated handshake supplies a fresh session UUID; the issuer ID is the
paired certificate fingerprint, not a name, address or shared network secret.
Grant-support advertisements are strictly validated and are only availability
hints, never permission or proof of correct execution by an adversarial node.

Schema 1 binds a canonical grant UUID, issuer fingerprint/session, exact executor,
existing job/task/attempt identities, concrete plan digest, original request context,
one capability/action and exact parameters. Physical resources must belong to that
executor. Inference binds one installed model/backend and optional owner model role;
it cannot fall back to another model/backend. Management/update operations and
custom capabilities without an enforcement contract cannot masquerade as grants.
Unknown authority fields, conflicting resources/models, invalid IDs, unsupported
actions, oversized inputs or unsafe limits fail before admission.

These are fixed request parameters. A named resource still resolves through its
node-owned adapter bindings; callers cannot redirect those local bindings. Saved
binding changes deny a pending/new grant until restart/replan. Raw local adapter
paths/device identifiers are not disclosed, and this is not proof of a physical
feed's identity or protection against a malicious node/local owner.

Authority is the intersection with the applied node policy. Relevant saved
permission/resource/model changes are checked before admission and while the
operation is pending. Revocation rejects pending/new granted work without waiting
for a node restart. New/widened saved scopes do not silently widen the applied
runtime: restart/replan remains required. Unrelated feature switches do not change
an operation's relevant permission fingerprint. A model/config/resource change is
treated conservatively until restart; this is not live pipeline migration.

## Durable consumption and acknowledgement

The node stores a consumed receipt before sending acceptance and before effects
start. It uses SQLite's transactional writer admission and FULL synchronization,
not an in-memory nonce cache. Competing runtime handles cannot both claim one
attempt. Reusing a grant UUID, the same/stale job/task attempt, or a higher attempt
while its predecessor is accepted/uncertain rejects rather than replays. A higher
attempt after a completed predecessor still requires separate explicit issuance;
this does not create a retry policy or authorize migration. Receipts are never
automatically pruned. Quota/full/damaged storage disables admission; opening a
damaged or unknown-version journal does not recreate or repair it.

The journal is bound to the stable node ID and lives beside the exact node config
as `CONFIG_FILENAME.grant-receipts.sqlite3`. Schema-1 receipts retain only
identity/digest/state metadata. The subsequent schema-2 increment also saves
bounded final reports/output before completion delivery, with admission-time
result reservations and explicit private reads. Parameters/original text/media
inputs are not separately copied, but output may itself contain sensitive input.
Keep it private with node data/backups; stop old runtimes and do not downgrade it.
Deleting/replacing it loses replay protection. This is not encrypted/tamper-proof
storage or defense against the machine's owner; see [retained results](RETAINED-RESULTS.md).

Acceptance confirms durable consumption and that execution has not yet started.
Completion confirms the same grant and scope digest, issuer/session/executor,
job/task/attempt and plan digest. The gateway accepts completion only after that
session's matching acceptance. Forged/duplicate ACKs, unpersisted success, ordinary
RPC responses and attempts to undo an ACK with a later `rejected` reply cannot
confirm granted execution. The reviewed execution path uses a direct journal
observer, not a broadcast listener: issuance is saved before dispatch, and matching
acceptance/final reports are saved before the RPC waiter receives confirmation.
Journal failure cannot yield confirmed success. This is not a distributed second
permission-to-start: effects can begin after the node sends acceptance, before EEF
checkpoints it. Results are bounded and stored once; ordinary job views redact
parameters and raw reports. Exact retained command retries inspect without issuing
another grant. Restart marks outstanding attempts unknown and never resumes them.

If acceptance cannot be sent, no operation starts, but its receipt remains consumed
and uncertain. Disconnect/restart does not revive it or an old lease. Lost replies
are not evidence that effects did not happen. Inspect a receipt/effect and make an
explicit recovery decision; never automatically issue fresh work to hide uncertainty.

## Limits and honest termination

Limits bound the RPC execution future and serialized result, not physical CPU/RAM
or every OS/backend side effect. The support advertisement says
`deadline_guarantee: rpc_future_only`. Application launches, already-sent network
requests and blocking/native/plugin/backend work may outlive cancellation of the
future. No immediate remote-stop, rollback, exactly-once effects, process-tree
termination, paused pipeline, cluster epoch or safe active/standby guarantee is
claimed by this increment.

Reviewed issuance anchors a monotonic deadline before the coordinator's durable
issuance write. Before consuming a grant, the node sends a fresh nonce-bearing
`grant_lease_probe` through its pinned authenticated session. The gateway replies
only for the exact pending issuance, executor, session and complete grant digest,
at most once. Its bounded `remaining_ms` cannot renew or change authority.
The two-second challenge timeout includes writer/queue delay. The node anchors
its deadline at local challenge **send**, not reply receipt, deducting the entire
round trip plus a conservative 5 ms + 1% remaining-budget margin. The durable
claim, ACK and execution share that fixed deadline. Queue residence, journal
writes and waiting for ACK cannot grant a fresh duration. An expired, late,
mismatched or unanswered proof denies admission before consumption/effects.
The gateway does not accept an ACK before issuing proof or after its deadline.

Updated nodes advertise `timing_mode: issuer_lease_challenge_v1`; missing or other
modes cannot receive new grants from updated EEF. There is no wall-clock or legacy
RPC fallback. Read-only inspection of existing compatible receipts does not require
the new mode and cannot reactivate a lease. Existing schema-1 grant/receipt journals
remain intact; update both roles before testing new execution on physical PCs.

Independent coordinator/node wall clocks no longer need alignment. Absolute grant
timestamps remain issuer-side audit/approval evidence; the executor never compares
them with its wall clock. Pending work still checks local policy and its own clock
discontinuities (over one second against monotonic elapsed time). This is not time
synchronization, hardware drift attestation, a suspend/resume guarantee or hard
real-time termination. Physical two-PC/clock acceptance remains a release gate.
Results after the deadline, execution errors, policy revocation or failed completion
checkpoints are conservatively unknown. Oversized results are omitted; effects may
already have completed and must not be repeated to retrieve them. Receipt timestamps
cannot move behind their acceptance timestamp during correction.

Optional node configuration, validated through the existing backend settings API:

```json
{"grants":{"max_receipts":4096,"max_journal_bytes":8388608,"max_duration_ms":30000,"max_result_bytes":65536}}
```

Defaults are shown. Owner bounds are 1-100000 receipts, 64 KiB-64 MiB journal,
1-30000 ms duration and 1 KiB-1 MiB result. The grant's own limits must be no wider
than the owner policy. Receipt/storage changes apply to a new runtime; saved
duration/result reductions also deny pending/new operations. SQLite page limits
bound the main journal; temporary rollback-journal overhead is additional. These
are protocol/storage/result bounds, not full workload resource accounting. Existing
node request budgets apply and recovery inspection does not consume an execution slot.

## Read-only recovery commands

```powershell
eefn grants receipt --id GRANT_UUID --json
```

This reads the exact node's existing journal whether or not its app/coordinator is
running. It starts no listener/model, changes no configuration, creates no journal
and never resubmits work. `accepted` may mean running or interrupted/uncertain;
inspection is not permission to replay. Output has `report_type: node_grant_receipt`,
`execution_requested: false`, `automatically_retried: false` and no execution inputs.

Equivalent guarded node-local API: `POST /api/commands/grants/receipt` with exactly
`schema_version: 1`, `expected_node_id` and `grant_id`. Wrong nodes/schemas, extra
authority fields, missing/damaged receipts and cross-site callers reject. No receipt
erase, replay, reset or recovery execution command is exposed. The separate
`requests execute` command accepts only an exact approved retained plan, never a
caller-supplied raw grant. Uncertain attempts cannot be hidden by Stop or resumed
through legacy job runners. The explicit `requests abandon` recovery branch can
close idle local tracking with an exact revision/grant and acknowledged uncertainty,
not by relabelling unknown effects as stopped/successful. Its `abandoned` state
retains issued authority and receipts, cannot resume/replay and survives restart.
The authenticated owner/origin decision is durable and idempotent. Separate explicit
history removal does not erase node nonce receipts. Lost-output/execution
reconciliation and strong remote termination remain later boundaries.

The original request node and local EEF owner can additionally use
`requests receipt JOB_ID --grant-id GRANT_UUID --json` through EEF. It selects only
the exact retained issuance and reads the independently authenticated executor's
journal under the same pinned issuer, including after session replacement.
The complete scope digest and bounded identity/timestamp metadata must match.
This uses a distinct reserved-control wire query, not an executable capability or
shared-secret fallback. Other origins, changed issuers/scopes, unavailable controls
and missing/damaged journals fail closed. Local read-only inspection validates the
same strict metadata. Neither read path creates, repairs, prunes or changes receipts.

`coordinator_request_receipt` reports explicitly set `execution_requested: false`,
`automatically_retried: false`, `reconciled: false`. Coordinator state is a revision
snapshot, not a write or resumed lease. This metadata-only command does not fetch
output/success; schema-1 receipts cannot supply them. The separate explicit
`result` command now reads schema-2 final reports without changing EEF tracking.
Missing evidence cannot prove no effects, and observed terminal state cannot prove
every external effect stopped. Inspection performs no reconciliation; local tracking
abandonment is a separate explicit command, not lost-output or execution recovery.
The separate [recover-result command](RESULT-RECOVERY.md) now saves exact retained
completed evidence in EEF's interrupted job journal. It preserves original
ACK/uncertainty and cannot dispatch unissued steps, renew authority or replay work.
See [command contract](COMMANDS.md#executor-receipt-inspection-through-eef-unreleased-after-alpha6).

Tests include real pinned-TLS client/gateway file-write execution, acceptance and
completion scope checks, unchanged owner markers after replay and node restart,
24 competing claims through independent journal handles, permission revocation
before effects, expired/future/incorrect grants, quotas, damaged/future journal
retention and read-only inspection. These are isolated local processes, not a
physical two-PC release acceptance or proof that an adversarial node obeys policy.
Fresh-lease units additionally cover synthetic +/-48-hour issuer offsets, exact
scope, expired/mismatched/widened replies, round-trip deduction, nonce replay,
local clock changes, cancellation and no native writes after ACK-wait expiry.
Gateway tests reject unproved/expired ACKs and renewal/old-session probes. They do
not change either PC's OS clock or prove safe suspend/partition/cluster behavior.
