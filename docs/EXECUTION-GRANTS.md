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
as `CONFIG_FILENAME.grant-receipts.sqlite3`. It retains bounded identity/digest/state
metadata, not parameters, original text, model output or media. Keep it with node
data and backups. Deleting/replacing it loses replay protection; this is not
tamper-proof storage or defense against the machine's owner.

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

Expiry/future-clock conditions reject on admission. A monotonic deadline starts
before the durable claim/ACK; execution cannot gain more time while waiting for an
ACK. Pending work checks local policy and wall-clock discontinuities. Results that
arrive after the deadline, execution errors, policy revocation or failed completion
checkpoints are reported conservatively as unknown. Oversized results are omitted;
the operation may already have completed and must not be repeated to retrieve them.
Receipt timestamps cannot move behind their acceptance timestamp during correction.
Coordinators/nodes currently need aligned clocks; the strict clock gate is a physical
acceptance item, not an automatic time-synchronization feature.

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
through legacy job runners; explicit reconciliation remains a later boundary.

Tests include real pinned-TLS client/gateway file-write execution, acceptance and
completion scope checks, unchanged owner markers after replay and node restart,
24 competing claims through independent journal handles, permission revocation
before effects, expired/future/incorrect grants, quotas, damaged/future journal
retention and read-only inspection. These are isolated local processes, not a
physical two-PC release acceptance or proof that an adversarial node obeys policy.
