# Durable trigger foundation (unreleased after alpha.6)

This extends the existing single-coordinator job journal and grant authority
checks. It is **internal infrastructure**, not a publicly enabled automation
feature. No CLI/API currently arms schedules, no background timer starts, no
fresh request is submitted automatically, and no grant is dispatched by this
increment. Existing definition authoring remains disabled; manual occurrence
reservation/explicit execution and terminal archiving remain available.

## Ownership and persistence

Additive `automation_schedules` and `automation_triggers` tables share the
existing exclusive journal lock, synchronous checkpoints and logical quotas.
A schedule binds an exact retained policy/digest, definition, generation,
runtime, issuer, declared interval and approval expiry. Its immutable arm receipt
can inspect later state, not refresh expiry or reactivate stopped/interrupted
work. There is at most one active schedule per definition.

A due claim commits a fresh immutable trigger and schedule cursor together. The
claim owns distinct request, reservation and execution IDs before any source job
could be created. It retains the policy's exact node/area origin with a fresh
request identity. It contains no cloned LM evidence, approval, task, assignment,
grant, acknowledgement or fabricated completion. Concurrent calls produce one
claim; repeated in-flight calls do not allocate replacement identities.

Ordinal/count/active-owner history and cross-claim identity uniqueness are
validated, including on startup. Missing, damaged, future or duplicated history
fails closed and is retained without repair, pruning or an implied refund.
Hashes check integrity/references; they are not signatures or remote attestation.

## Timing, lifecycle and stop authority

The internal schedule states are waiting, in-flight, stopped, interrupted and
exhausted. The planned execution cadence is **fixed delay**: first due after one
declared interval; another due time only after observed, validated terminal
resolution of the previous occurrence, plus one interval. It is not catch-up
replay or an overlapping fixed-rate job queue.

Claims require current runtime/issuer/policy/generation, a full remaining
duration allowance and the exact due window. A missed window of one whole
interval, a clock below the last durable observation, or changed policy interrupts
without claiming work. Final clock/policy/window checks before transaction commit
roll back a claim if time changes or its authority expires. Run/time exhaustion
cannot shorten a run or create an extra claim. Arms/claims also check the policy's
permanently consumed reservation allowances, not merely one schedule's local
claim count. A replacement arm cannot reset an exhausted policy's budget.
Private injectable-clock helpers
serve tests; future production callers have actual-clock entry points, not
client-supplied timestamps.

An unadmitted claim with no retained job is **uncertain**, not proof that nothing
happened. Settlement requires the existing reservation plus actual validated
completed or unissued-cancelled job evidence, including archived terminal proof.
Failed, interrupted, abandoned or partially issued work never permits replacement.
Claim ordinals and occurrence run/duration/result consumption are not refunded.

A strict revision/receipt-bound stop checkpoints future-issuance denial. It does
not claim that a durably issued grant was unsent, physically terminated or remotely
cancelled. Already-issued work still needs existing lease/enforcement/result
recovery and explicit job lifecycle handling. Exact stop receipt reuse is
inspection, even after expiry or a quota reduction.

Reservations for trigger-owned requests automatically bind the recorded trigger
and its reserved receipt. A different receipt or ordinary direct approval cannot
bypass that binding. The execution receipt must also be the one recorded by the
claim. Start and every existing reviewed grant-issuance checkpoint check the
schedule under the same SQL mutex as their write. Stopped, interrupted, expired
or old-runtime schedules deny new unissued work. Historical inspection, terminal
archiving and completion recording remain possible without reactivating it.

Opening the journal interrupts retained waiting/in-flight schedules before a
runtime could resume scheduling. Claims and their original IDs survive; restart
does not create jobs, replay triggers, re-arm authority or automatically reconcile
unknown outcomes. This is single-coordinator safety, not election/replication or
stale-leader protection across coordinators.

## Storage

Schedules reserve a total of **4096 logical bytes per row**, including their
bounded record and stop/interruption headroom. This permits those lifecycle writes
after lowering a logical quota, without claiming physical disk space is available.
Trigger rows consume their actual bounded UTF-8 JSON payload and a shared history
count. Schedule/cursor plus trigger insertion is atomic; an insert/count/byte quota
failure advances neither. All job/definition/policy/reservation/archive writes
include the new records and headroom in their accounting.

Storage adds `schedule_count`, `schedule_bytes`, `schedule_reserved_bytes`,
`trigger_count` and `trigger_bytes`. Existing count/byte field meanings remain.
Unreserved trigger-owned partial source jobs cannot be silently removed; their
lineage is needed to distinguish partial admission from unknown dispatch. Resolved
reserved jobs use the existing exact occurrence archive path. Retention/compaction
and explicit unresolved-claim handling remain necessary follow-up work.

## Still required before public activation

Connect shared owner enable/stop/inspect commands to the engine's admission/
shutdown gate and a bounded runtime-owned timer task. Exercise the subsequent
internal fresh-request bridge through those adapters, validate current origin/
provider/disclosure/model/grant bounds and retain actual partial outcomes. The
bridge uses the existing compiler/reservation/driver and must never relabel old
model inference as new evidence.
Integrate monotonic sleep/suspend discontinuity detection; the core's durable-wall-
clock checks alone do not detect every intervening clock change.

Test lost replies, stop during admission and between issuance steps, worker
shutdown, provider disappearance, partial compilation/reservation/dispatch,
lease/native termination, backlog/backpressure, disk failures and physical two-PC
behavior together. Define an explicit no-work/recovery path for claims without
terminal evidence; a missing job cannot authorize automatic retry. Public timer
activation stays unavailable until the bridge/worker/adapter path is validated.
Full lightweight interpreter bootstrap, heavy planning, measured resource budgets,
peer/media/cluster, non-coder onboarding and installer/Defender/Authenticode
acceptance remain in scope. Stable 0.4.0 is not claimed.

## Evidence (2026-10-12)

Twelve added tests bring both final workspace/all-target feature modes to **489**:
EEF library 197, owner CLI 11, installer 6, EEFN library 269, node CLI 6.
Eleven journal tests cover one claim under sixteen concurrent callers, no source
mutation or automatic job creation, insertion/count/byte/final-clock rollback,
missed/backward/changed authority, uncertain unadmitted claims, immutable stop/
arm receipts, no direct-approval or receipt-substitution bypass, stop/issuance
fences, actual unissued cancellation/archive settlement, fixed delay and permanent
budget consumption, time exhaustion, lowered-quota restart/stop, damaged/future/
missing history, duplicate active ownership and cross-claim identity aliases.
The shared-contract test proves ordinary delegation serialization still omits
the new optional link and rejects invalid/extra-field trigger references.

The fresh default executable/example build, formatting, diff and backend/browser
source guard pass. An initial 488-test run preceded the final ownership invariant;
the final 489-test suites also include the subsequent exhausted-policy guard.
Initial compile errors (a private digest helper and a test-only borrow conflict)
were corrected before those runs; they are not counted as passing evidence.

Fresh public regressions pass occurrences/native execution/archives
(`automation-occurrences-Lf22nd`), definitions/policies
(`automation-definitions-ggPMGn`), input/answers (`request-input-A8wVkQ`) and
broader reviewed execution/recovery (`request-review-kii2bf`). The occurrence
fixture additionally confirms public use creates zero schedules/triggers and
zero schedule headroom. Its actual native `system.info` execution is manual,
not scheduled. The review fixture uses fake inference and does not establish
real interpreter quality. Prior intermittent input/busy failures remain unproven.

The scheduler tests are internal journal/clock/compiler checks, not a running
timer, native scheduled workload or physical two-PC acceptance. No runtime worker
or public activation adapter is enabled. Installed nodes, published versions/
installers, update feeds, browser assets and model files remain unchanged.

## Internal fresh-admission bridge (subsequent increment)

The internal runtime bridge now creates a fresh ordinary explicit owner-directed
request from the fixed input, checks the current origin registration/age/area and
exact providers/issuer/grant bounds, and calls existing ordinary compilation,
atomic occurrence reservation and reviewed execution. The source goal title may
describe the fixed operation, but its LM interpretation/preparation history,
task identities, approval and execution evidence are never copied. Origin moves
cannot retag the old scope. This service is not yet called by a public owner
adapter or runtime timer task; compiling it does not prove native scheduled work.

A durable one-shot launch marker precedes any source admission. Sixteen concurrent
begin calls authorize one launch; repeated marked callbacks only inspect actual
retained state, including after a crash with no source job. Preexisting source or
reservation receipts cannot become implicit continuation. Marker write/clock
failure rolls back before source work. Stop and changed policy also fence source
insertion and new compilation under their existing SQL/admission checkpoints;
historical receipt reads and explicit cancellation remain possible.

A failed/stale callback may interrupt **only its own current claim**. It cannot
overwrite owner stop or interrupt a successor claim. Partial admission resolves
only from an actual retained cancelled source with no approval/execution, or the
existing reserved occurrence's terminal proof. Missing jobs stay unknown and do
not permit automatic retry. Unreserved cancelled sources remain retained: there
is no silent deletion, fabricated reservation or claim/budget refund. A missing-
source recovery/closure command and safe partial-source compaction are still open.

Ten added journal tests bring final workspace/all-target suites to **499** in
both feature modes (EEF library 207, owner CLI 11, installer 6, EEFN library 269,
node CLI 6). They cover late stop/admission/compilation, policy revocation,
origin substitution, retained partial cancellation and missing-history refusal,
one-shot concurrent launch, rollback, preexisting/stopped launch refusal,
marked-but-unadmitted restart and stale callback/successor ownership. The fresh
default binaries/examples, formatting, diff and backend/browser source guard
also pass. An initial 498-test run preceded the final stale-callback test.
These are journal/compiler/clock tests, not native bridge or timer acceptance.
Public activation, the bounded timer manager/monotonic discontinuity handling,
stop/worker/network/disk fault acceptance and the full remaining roadmap stay open.

Fresh public regressions also pass native manual occurrences/archives
(`automation-occurrences-SdlG5T`), definitions/policies
(`automation-definitions-oAk0yu`), input/answers (`request-input-Mqi85z`) and
reviewed execution/recovery (`request-review-xiFubs`). The native execution is
manual, not through the new internal bridge; the review fixture uses fake
inference. These regressions do not prove native bridge/automatic timer behavior
or interpreter quality. Earlier intermittent input/busy failures remain unproven.
