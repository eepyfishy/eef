# Owner-controlled automatic schedules (unreleased after alpha.6)

This connects the durable trigger/one-shot launch foundation to a bounded
runtime timer task and guarded owner commands. Fixed-input one-shot templates
can now create fresh ordinary requests, compile them, reserve declared authority
and execute through the existing reviewed grant driver. This is not general
semantic automation, a second runner, continuous media or stable 0.4.0.
Nothing activates merely by saving a definition/policy or starting EEF.

## Owned periodic journal worker (subsequent increment)

Periodic schedule reads, due claims, settlement and checkpoint-error housekeeping
now run on a blocking worker, keeping their synchronous journal lock/validation
off async workers. The timer still awaits each checkpoint serially: at most one
periodic callback is queued/running, with no extra schedule/work queue, cached
authority, changed poll interval or new workload executor. Launching remains in
the existing compiler/reservation/grant driver with its existing bounded slots.

A guard owns that queued/running callback independently of its async waiter.
Shutdown closes admission, drains the guard, then interrupts schedules; dropping
a waiter does not pretend the blocking callback was aborted. A callback that
starts after closing skips journal processing, and a returned claim is not
launched after closing or removal from the active schedule set. This follows
Tokio's [blocking task lifetime contract](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).
Worker count is bounded, not guaranteed I/O/shutdown duration. A stuck storage
operation can still delay shutdown; no timeout falsely confirms its termination.

Current-row validation, transactional final time/policy fences, overlap/refund
rules, identities and no replay remain. A normal checkpoint error gets its one
existing `checkpoint_failed` attempt inside the worker. Lost/panicked workers
need inspection, not a repeated write or new claim. Trigger launch, owner command
inspection and other synchronous journal paths are not offloaded by this slice.
It is not the general DB actor, strong stop or completed responsiveness gate.

The six focused timer tests pass, including the three new worker cases: a held
real journal lock leaves the single-thread async executor available; shutdown
drains the owned checkpoint and refuses extra/later workers; a cancelled waiter
retains its queued callback until closing skips it with zero journal changes.
Default and dashboard-free workspace/all-target suites each pass **612 tests**
(EEF library 234 plus one ignored opt-in profiler, owner CLI 16, installer 6,
node library 345 and node CLI 11).

The fresh default build and unchanged ordinary scheduling fixture
`automation-schedules-zU1995` pass, including the final explicit replacement
activation that failed in telemetry-only `automation-schedules-57c0Sr` with
confirmed stale heartbeat age and zero issued attempts. The initial new-test
compile failed on an incorrect event-module import; only that test import was
corrected before the passing focused run. It is not counted as a pass.
Final fresh default executables/examples build and public regressions
`local-pipelines-kTb27e` and `process-lifetime-CnDq3Q` pass, with the complete
verification-chain exit 0. Formatting, diff and the backend/browser source
boundary pass. These local passes do not isolate every previous timing failure
or prove loaded-system,
physical network, general journal responsiveness, strong stop or stable release
acceptance. Earlier failed fixtures remain failed evidence.

## Journal admission and diagnostics (subsequent increment)

Trigger overlap and remaining allowance checks share one fresh fully validated
occurrence scan under the existing journal connection lock. Schedule history
validation loads each policy/definition once within that invocation; schedule
and trigger point readers share the same predicates. No validated state is cached
across a write, checkpoint or call. Row identity, digest, headroom, ordinal,
generation, context, archive, no-refund and lineage checks remain. Existing
due-window, policy validity and final transactional commit fences still apply.

A timer checkpoint failure now records `checkpoint_failed`, not `job_unresolved`.
The latter remains valid historical evidence and is still used for an actual
unresolved in-flight job. Neither reason confirms remote stop or no effects,
refunds authority, rearms the schedule or permits retry/migration.

Opt-in development profiling reads an existing isolated fixture database with
SQLite read-only flags. It prints phase timings only, not records/credentials:

```powershell
$env:EEF_PROFILE_JOURNAL = 'PATH_TO_EXISTING_FIXTURE_DB'
cargo test -p eef --lib profile_read_only_timer_journal -- --ignored --nocapture
```

Runtime claim phase timings require the explicit development logging filter
`RUST_LOG=warn,eef::jobs::automation_schedules=debug`. Logs contain only schedule
IDs, phase names and elapsed microseconds. The ignored profiler is not normal
test acceptance or an installed command. Measured speedups do not establish a
timing guarantee or clear short-interval/loaded-system release acceptance.

## Commands

First create/inspect an ordinary compiled source, save its disabled definition
and approve an exact bounded policy using the existing command families. Then:

```powershell
eef automation-schedules enable --policy POLICY_UUID --policy-sha256 POLICY_HASH --operation-id SCHEDULE_UUID --json
eef automation-schedules show SCHEDULE_UUID --json
eef automation-schedules list --limit 25 --json
eef automation-schedules stop SCHEDULE_UUID --revision CURRENT_REVISION --operation-id STOP_UUID --json
eef automation-schedules resolve-refusal SCHEDULE_UUID --revision CURRENT_REVISION --trigger-id TRIGGER_UUID --refusal-sha256 REFUSAL_HASH --operation-id RESOLUTION_UUID --json
```

Only the local coordinator owner endpoint
`POST /api/commands/automation-schedules` accepts `{schema_version:1,
expected_runtime_id,command}`. Node-local APIs/remote management cannot grant
this authority; foreign browser origins are rejected. No arbitrary parameters,
permission override, force/renew/retry or new policy is accepted. Enable checks
current origin registration/age/area and exact providers/issuer/grant bounds
before durable activation. The ordinary compiler still enforces disclosed
providers and matching fixed input; reservation independently requires the exact
approved operations/node/resource/model/backend bindings. Changes cannot retag
origin or silently rebind executors/models.

Enable **permits future automatic execution** within the existing owner policy,
not merely a record-only checkpoint. Its wrapper therefore removes the earlier
automation adapters' false/no-timer/no-dispatch claims. `timer_active` is a
current manager/schedule snapshot, not provider readiness or completion. For a
lost/corrupt/negative post-commit reply it remains unknown (`null`), and
`outcome_unknown` stays true. Preserve the original command/receipt and inspect
it; exact enable reuse is read-only and does not renew/re-register old authority.

Show reports current trigger/job identity/status without invoking inference or
execution. List pages contain only strict bounded summaries, no input parameters
or source/terminal snapshots. Private owner reports should not be published.

## Runtime behavior

The first trigger is due after one interval; another due time requires actual
validated completed/unissued-cancelled evidence and is delayed by one interval
from settlement. There is no catch-up, overlap queue, hidden continuation or
automatic retry after partial/unknown work. A fresh structured owner-directed
request is created each time; original LM evidence, task IDs, approval and
execution history are never copied. Native leases, node permissions, scheduler
admission, existing durable issuance/ACK/completion and explicit recovery remain
the execution boundary. Run/duration/result consumption is permanent, and new
schedule IDs cannot reset a policy's exhausted budget.

Defaults (configurable backend settings, applied on explicit EEF restart):

- `automations.poll_ms`: 200; allowed 50–1000.
- `automations.max_active`: 8; allowed 1–32.
- `automations.max_launches`: 2; allowed 1–8 and no more than active capacity.
- `automations.launch_timeout_ms`: 30000; allowed 1000–60000.

The manager tracks only explicitly enabled current-runtime schedules, bounds
concurrent admission tasks, and uses the existing driver for workloads. Full
launch capacity delays an unclaimed tick, never creates an unbounded backlog;
missed-window rules can interrupt it. A launch timeout interrupts for inspection,
not rollback, remote cancellation or permission to replay. Durable claims/launch
markers remain after uncertain admission. CPU/RAM/bandwidth/power budgets and
hard native termination are not established by these concurrency bounds.

An independent bounded clock-monitor thread samples without journal processing.
Backward wall time, wall/monotonic delta disagreement above 500 ms, or a monitor polling
gap above `max(2000 ms,10*poll_ms)` conservatively interrupts active schedules.
This is bounded engineering detection, not a claim to detect every hardware
sleep/clock condition. Journal checks still enforce due-window/current-policy/
expiry/commit-clock constraints. Unexpected task/SQL/validation failure retires
management without catch-up; stopped/expired/interrupted authority fences new
source admission, compilation, reservation and every reviewed grant issuance.

Stop first checkpoints future-issuance denial, then requests ordinary lifecycle
stop for a known current nonterminal job. It distinguishes `job_stop_requested`
and observed status; no native process/device is claimed physically stopped.
Already-issued work may finish or need explicit result recovery. Exact stop
receipt reuse does not re-request job cancellation. A stale stop revision must
be inspected, not silently replaced with a new receipt.

Shutdown closes manager admission and checkpoints interruption before aborting
timer work. Restart never re-registers/re-arms stored schedules or replays old
triggers; explicit owner activation requires a new valid policy/receipt and
resolved earlier work. Missing source jobs stay unknown, not no-work proof.
General partial-source/no-work closure, payload compaction and first-time friendly
automation authoring remain usability work. No browser visual/controller change,
model download, installed-node update or release/feed replacement occurs here.

## Evidence and remaining acceptance (2026-10-12)

Five added tests bring final workspace/all-target suites to **504** in both
feature modes: EEF library 211, owner CLI 12, installer 6, EEFN library 269,
node CLI 6. They cover strict correlated enable/summary contracts, private/extra
field refusal, no hidden retry/permission widening, mandatory CLI receipts and
stop revision, bounded configuration and conservative clock/gap detection.
Fresh default binaries/examples, formatting, diff and backend/browser source
guard pass. The first build's collection type-inference error was corrected;
that failed build is not counted as passing evidence.

Public native fixture `automation-schedules-yn74OE` uses two pinned-TLS logical
nodes on one PC with no models/UI controller. Sixteen concurrent enables create
one schedule; three fresh native `system.info` jobs complete automatically with
original source history unchanged. It refuses budget reset and foreign/node
authority, stops before due and after one completion, prevents the next run after
policy revocation, retains interruption across restart without re-arm/catch-up,
and handles dropped/substituted/negative-after-commit enable replies as uncertain
while exact inspection finds one actual occurrence. The earlier shorter
`automation-schedules-YdNdmD` fixture also passed before the additional cases.

These are local native one-template checks, not physical two-PC, real semantic
model quality, general/heavy pipelines, measured full resources, hard native
termination, all worker/network/disk/power races, peer/media/cluster or installer/
Defender/Authenticode acceptance. Those full roadmap gates remain in scope.
Stable 0.4.0 is not claimed.

Current-build manual occurrence/archive regression (`automation-occurrences-49gsFD`),
definitions/policies (`automation-definitions-6vgTMy`), input/answers
(`request-input-oDPwa4`) and reviewed execution/recovery (`request-review-VDmOHj`)
also pass. The last uses fake inference, not real semantic quality.

The first occurrence regression invocation (`automation-occurrences-5gLDQt`)
terminated with exit code `-1073740791` and no normal test result. It is **not**
counted as passing. A scoped process check found no fixture processes remaining;
no matching application error event was found in the checked interval. A fresh
rerun passed without source changes. This does not prove the cause or a fix.
Earlier intermittent input/busy failures also remain unproven. Installed nodes,
published versions/installers, update feeds, browser assets and model files are
unchanged; no Defender/Authenticode safety claim follows from these tests.

## Explicit pre-launch refusal recovery (subsequent increment)

The existing bridge can now checkpoint factual `refused_before_launch` evidence
when its current-runtime preflight returns an error before the durable launch
marker. The transaction verifies the exact active immutable trigger, absent
marker, and no source job/reservation, then interrupts that schedule. Absence by
itself is never proof: marked-but-missing, partially admitted or uncertain issued
work cannot use this path. Hashes bind retained records, not remote attestation.

Inspect `show`, retain `schedule.refusal`, and explicitly use `resolve-refusal`
with the exact schedule revision, trigger ID, refusal digest and operation ID.
Concurrent identical commands produce one retained resolution. Conflicting or
stale references fail; lost/negative/corrupt replies remain uncertain and require
exact inspection/reuse, not a replacement operation or a force override.

Resolution changes the historical schedule to `resolved`, not waiting. It
creates no job, timer or grant, preserves all claim identities/counts and any
stop receipt, and refunds no reserved occurrence allowance. A refusal before
reservation has consumed no occurrence allowance in the first place. A separate
explicit enable may create a fresh schedule only while current policy, provider,
origin and permanent allowance checks pass. Restart never replays the old claim.
Lifecycle headroom allows resolution under a lowered logical quota; it is not
physical disk reservation. General partial/issued-work recovery remains open.

Eight added tests bring final workspace/all-target suites to **512** in both
feature modes: EEF library 218, owner CLI 13, installer 6, EEFN library 269,
node CLI 6. They cover concurrent exact resolution, missing/marked/partial refusal,
stale references and transactional rollback, restart/lowered-quota/stop receipt
preservation, corrupted proof/history, correlated passive reports, mandatory CLI
references, and independent monitor sampling during ordinary processing pauses.
The passive owner confirmation also checks the nested refusal digest/lifecycle
and rejects contradictory job/launch evidence even with a recomputed outer hash.
Fresh default binaries/examples, formatting, script syntax, diff and backend
boundary guard pass.

Extended native fixture `automation-schedules-g9fgE0` passes the earlier cases
plus actual origin disconnection before launch, explicit proof-bound resolution,
no replay after reconnection, and a fresh separately enabled native completion.
It is two logical nodes on one PC, not physical two-PC acceptance.

Earlier extended runs `automation-schedules-bbadFt`, `automation-schedules-A26GJQ`
and `automation-schedules-TAM4Be` failed after replacement admission because the
timer included ordinary journal processing in its long-gap measurement.
Diagnostics captured a 2366 ms processing-loop gap and a 556 ms checkpoint.
These runs are not counted as passing. The independent monitor separates that
work from clock/suspend samples; all existing thresholds, due-window, expiry and
no-replay checks remain. This does not prove every suspend/resource race or fix
the separate older unexplained process exit.

A later full-suite run rejected an existing test archive timestamp that could
precede its actual cancellation checkpoint under parallel load. The fixture now
dates archive/settlement no earlier than that cancellation. Production evidence
validation is unchanged; the failed run is not counted as passing.

Fresh regressions pass manual occurrence/native execution/archive
(`automation-occurrences-r5BOzh`), definitions/policies
(`automation-definitions-VRWStm`), input/answers (`request-input-FQpbRn`) and
reviewed execution/recovery (`request-review-EFdrAP`). The review fixture uses
fake inference. These runs preceded the final passive nested-proof confirmation
hardening; they use the same execution/recovery code, not a new readiness claim.

Final native `automation-schedules-mlHTYT` passes against the final rebuilt
binaries, including dropped, substituted and negative-after-commit resolution
replies: each stays unknown to that client, exact reuse finds one resolution,
and no old claim replays. The preceding `automation-schedules-9dV8KM` invocation
exited 1 without a normal result/failure report; its journal retained the refusal
without resolution or replay and no fixture processes remained. No matching
application/Defender event appeared in the checked interval. It is not a pass,
and the subsequent traced rerun does not prove its cause or a fix.

## Journal cost and truthful checkpoint failure (2026-10-13)

The previous source checkpoint reproduced ordinary one-second scheduling failures
in `automation-schedules-DVNvnb` and isolated `automation-schedules-m1C9Ta`, both
after two completed jobs. Investigation used the latter database strictly
read-only. Its SHA-256 remained
`25166cc8c4dcc20c9c7f0bc6649b3990234c097d4d2e3bb08e77876f3d7d868c`.
Initial occurrence validation took roughly 169-210 ms for only two records.
Combining the two admission checks measured 208-255 ms versus 371-493 ms for the
separate checks in a final focused run. These are local debug-build samples,
not latency guarantees, release-mode measurements or physical-device acceptance.

The new joint gate preserves unresolved/live overlap refusal, cancelled/archived
resolution, exhausted allowance/no refund and later corruption detection. Tests
check that inspection changes no database rows. Per-invocation schedule dependency
reuse retains row/digest/headroom/time/context/identity/history predicates from
point readers; all current and commit authority checks remain fresh. A checkpoint
error now records `checkpoint_failed` rather than incorrectly implying an
unresolved job. Historical `job_unresolved` remains accepted.

An initial negative-test SQL statement assumed an `ordinal` column that is
actually inside the retained JSON. The test was corrected to address that JSON
field; production schema and authority were unchanged. A direct profiler launch
without its runtime DLL search path exited `0xc0000135` without a harness result;
the properly configured read-only launch passed. Neither failed setup is counted
as acceptance or used to explain older unrelated native exits.

Performance acceptance remains open. Joint-scan-only `automation-schedules-e5kodJ`
failed before its first durable claim. Traced `automation-schedules-lmyq8R` kept
two completed jobs, then refused its third claim: phase timings showed about
805 ms inside the claim before the final fence refused it. Subsequent dependency-
reuse `automation-schedules-fKKHCI` completed one job, then retained `missed_trigger`
with no next due time. Slow checkpoints/lock and processing contention still
require investigation. These are failed public regressions, not evidence the
one-second timer gate is fixed. No timing window, run budget, expiry, no-catch-up
or no-unsafe-replay condition was relaxed.

Final workspace/all-target suites pass **604 tests** in both feature modes:
EEF library 230 (plus one ignored opt-in profiler), owner CLI 16, installer 6,
EEFN library 341, node CLI 11. Passing units do not clear the failed public
scheduling gate or complete the full bootstrap/media/resource/peer/cluster/
physical/signing and stable 0.4.0 requirements.

Fresh final workspace binaries/examples, formatting/diff, JS syntax and the
backend/browser source guard pass. Final public `local-pipelines-dOFmtu` passes
finite local execution, one-parent child admission, permission loss, known failed
native stage, bounded five-second recurrence and unchanged histories/no rearm
after restart. This does not clear ordinary one-second scheduling acceptance.

Preceding `local-pipelines-SuDk94` exited `0xc0000409` without a normal result or
failure report; read-only inspection found zero jobs and no remaining fixture
processes, and recent matching Application events did not establish a cause.
Bounded private harness-exit/phase diagnostics were added before the subsequent
isolated passing run. That passing run does not explain or prove a fix for the
native exit. No installed runtime/node, release/feed, installer, UI, model or real
shared service changed by this source checkpoint.

## Request-local history readers (2026-10-13)

Read-only schedule list/find/get and trigger get/job now return data from the
complete validated history view created for that request, instead of validating
the same records again after the history scan. Exact enable-receipt lookup uses
that same view. Each invocation loads and checks current rows; the view is neither
execution authority nor a cache across calls/writes. Global identity/count/lineage
validation remains mandatory even when the requested ID is missing.

The new reader case matches batch results against point readers, checks zero
database changes during inspection, observes a later interruption and rejects
subsequent trigger-row damage across every reader (including a missing-ID lookup).
On the unchanged read-only failed fixture, back-to-back list measurements were
3.5-4.9 ms for one view versus 7.8-8.6 ms for validation followed by reload. Absolute
timings differ from earlier samples; these numbers do not prove a hardware/load
guarantee or isolate every contributor to the previous latency.

Final workspace/all-target suites pass **605 tests** in both feature modes:
EEF library 231 plus one ignored opt-in profiler, owner CLI 16, installer 6,
EEFN library 341, node CLI 11. Fresh default binaries/examples, formatting/diff
and the backend/browser source guard pass.

Unchanged public ordinary scheduling passes `automation-schedules-g6UcnO` and
fresh final-binary `automation-schedules-GGKXkH`, including one-second native runs,
concurrent enable receipts, stop/revocation, restart, uncertain replies, actual
pre-launch refusal, resolution and explicitly fresh replacement. Final public
`local-pipelines-Ee7aNY` also passes its finite/admission/native/restart/recurrence
cases, with complete regression-chain exit 0. These are two logical nodes on one
PC, not physical two-PC or heavy-load guarantees. Earlier failures remain recorded;
the isolated passes do not establish their complete cause or erase resource-
contention/clock/physical release gates. No timing window or authority limit changed.

No installed runtime, physical node, release/feed, installer, UI, model or shared
service changes follow. Full 0.4.0 acceptance remains incomplete.
