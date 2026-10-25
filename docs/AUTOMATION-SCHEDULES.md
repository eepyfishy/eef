# Owner-controlled automatic schedules (unreleased after alpha.6)

This connects the durable trigger/one-shot launch foundation to a bounded
runtime timer task and guarded owner commands. Fixed-input one-shot templates
can now create fresh ordinary requests, compile them, reserve declared authority
and execute through the existing reviewed grant driver. This is not general
semantic automation, a second runner, continuous media or stable 0.4.0.
Nothing activates merely by saving a definition/policy or starting eef.

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
(eef library 234 plus one ignored opt-in profiler, owner CLI 16, installer 6,
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

### Read-only command inspection worker (subsequent increment)

Owner `automation-schedules show/list` now uses a separately owned blocking
inspection callback. There is at most one queued/running inspection, independent
of the one periodic checkpoint slot; excess reads fail busy instead of creating
an unbounded queue or disabling the timer. No new implicit command retry is added.
Schema, current-runtime and command validation still precede worker admission.
Enable, Stop, refusal resolution and execution authority are unchanged.

Show reads the fully validated schedule and its linked job under one journal
lock and request-local history view, eliminating a second history scan. The
view is never cached across calls or used as permission to execute. List keeps
the existing summaries, pagination and private-field exclusions. A shared report
builder preserves the current response and strict confirmation contract for both
read-only and management commands. Active timer membership is observed after
the journal lock is released; these are snapshots, not readiness/stop proof.

The same ownership guard tracks periodic and inspection classes separately.
Shutdown closes both admissions and drains queued/running callbacks before
interrupting schedules. A late inspection result after closing is rejected,
not presented as current readiness. Cancelling an async reader does not forget
its blocking callback. Storage stalls can still delay completion/shutdown.

Focused automation tests pass 64 cases with one ignored profiler, including
independent single-flight slots, a held real journal lock with an available
single-thread async executor, rejected late results and cancellation/shutdown
ownership for both callback classes. Snapshot checks retain zero writes, observe
later interruption and reject damaged global history even for a missing ID.
The empty read page retains the report contract and rejects management commands.
Initial refactoring emitted unused-initialization warnings; those were removed
without changing behavior. Final verification is recorded below.
The first fresh read-worker build passes ordinary scheduling
`automation-schedules-ccD257`, but `local-pipelines-U7orKe` fails the unchanged
45-second recurrence bound. Read-only coordinator inspection retains one
completed automatic job and a second running job with one issued grant but no
saved ACK/result. The node retains that same grant as persisted completed/success
with 58 ms between acceptance and final checkpoint. This is missing coordinator
evidence/timely settlement, not proof of replay, absent effects or a passed run.
Checkpoint logs reach about 6.2 seconds; the complete delay cause remains unproven.

Investigation keeps the failed coordinator DB SHA-256 unchanged:
`7413089542155ca9b6ce75928e86deebbc0e2e39e0d1190066fa52215af90972`.
The opt-in read-only profiler measures occurrence validation at roughly
0.38-0.62 seconds and joint admission at 0.44-0.51 seconds in initial samples.
Occurrence admission now reuses only the resolution bit of the retained plan it
already fully validated in that invocation. It does not retain another payload,
cache state across calls/writes, bypass row/history/budget checks or issue work
from a read view. Existing tests compare that bit against point validation,
observe later cancellation/archive/exhaustion and reject later corruption.
The automation-focused suite still passes 64 cases with one ignored profiler.
Post-change joint samples of 0.43-0.48 seconds overlap the initial range; they
show no established absolute timing guarantee or complete cause/fix. No bound
is increased and the failed fixture remains failed evidence.
The subsequent duplicate-plan-reuse build fails ordinary scheduling
`automation-schedules-pNjBSp`: one automatic job completed, then a claim crossed
its final commit-time fence and retained `checkpoint_failed` without a second
claim. Logged checkpoints reach about 1.7 seconds. The chained pipeline test
did not run. This is not a passing timing gate or proof that reuse solved the
settlement delay.

Final default and dashboard-free workspace/all-target suites each pass **615
tests**: eef library 237 plus one ignored opt-in profiler, owner CLI 16,
installer 6, node library 345 and node CLI 11. The snapshot regression also
checks the exact partially admitted job and immutable trigger origin, rejects
substituting the original template job, observes a subsequent cancellation and
preserves zero database changes during each inspection. Fresh default workspace
binaries/examples, formatting/diff and the backend/browser source guard pass.
The final verification chain exits 0; it does not rerun or clear the failed
public timing gates. The failed coordinator DB hash remains unchanged.

Source inspection at this checkpoint identified another synchronous path:
`eefn::server` held its shared pending-request mutex while invoking the durable
grant observer, and `eef::request_execution::AttemptJournal` performed journal
validation/writes synchronously. This could stall async workers and unrelated
correlation management. It is a confirmed code path, not proof of the complete
cause of either failed fixture. Moving it requires bounded, shutdown-owned
callbacks, durable ACK-before-completion ordering and exact session correlation;
detached writes, replay or extending issued lease deadlines are not solutions.
The subsequent [owned report-worker increment](GRANT-REPORT-JOURNAL.md) moves
these saves off async workers, releases the shared correlation mutex during
storage, fences late ACKs and drains callback/result ownership on Stop. Fresh
public ordinary scheduling, pipelines and native lifetime pass in that source
checkpoint. Other paths and loaded-system/physical timing acceptance remain open.
Other command/launch journal paths, loaded-system/physical acceptance and the full
stable 0.4.0 requirements remain open. No installed node, release/feed, UI or model
is changed by this source increment.

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

Defaults (configurable backend settings, applied on explicit eef restart):

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
feature modes: eef library 211, owner CLI 12, installer 6, eefn library 269,
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
feature modes: eef library 218, owner CLI 13, installer 6, eefn library 269,
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
eef library 230 (plus one ignored opt-in profiler), owner CLI 16, installer 6,
eefn library 341, node CLI 11. Passing units do not clear the failed public
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
eef library 231 plus one ignored opt-in profiler, owner CLI 16, installer 6,
eefn library 341, node CLI 11. Fresh default binaries/examples, formatting/diff
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

## Claim-local validation frames (2026-10-18)

Due-claim eligibility now reuses the fully validated history's policy,
definition, generation and schedule data for that read phase, instead of
reloading and recompiling those same dependencies again. The pre-write view is
discarded before staging. The staged transaction constructs a separate complete
validated view before checking current authority and the final clock/window/time
budget. Nothing is cached across writes or calls; the view itself is not work
authority. Other lifecycle point readers retain their existing checks.

Commit-fence errors distinguish a backward clock, an elapsed trigger window,
changed/expired authority and insufficient remaining execution time. They still
roll back the schedule cursor and every staged trigger identity. The timer's
existing error handler may retain `checkpoint_failed`; it never relabels the
failure as successful work or authorizes catch-up. No timing window, interval,
run/resource allowance, lease, expiry, inspection predicate or replay condition
is widened. No schema, command or wire version changes.

New tests compare snapshot eligibility with the original point reader for time,
runtime, issuer, revocation and source removal. An isolated SQLite trigger
revokes or corrupts policy after staging; the new complete post-write validation
refuses the claim and rolls back both the trigger and policy change. Backward/
late/time-budget cases retain their original unclaimed state and report distinct
causes. Existing count/byte/insert faults, corruption, concurrency, lineage,
budget, stop and restart tests remain applicable.

The public scheduler harness now writes `timer-checkpoints.json` on success or
failure. With `RUST_LOG=warn,eef::jobs::automation_schedules=debug`, it summarizes
the existing phase logs (sample count, median and maximum microseconds since
checkpoint start), plus slow-checkpoint warnings. No source/prompt/operation
payload or credentials are copied into that diagnostic. Missing phase logging is
reported as absent, not as zero latency. Diagnostic write failure cannot skip
fixture cleanup. These bounded log samples are not a performance guarantee or
a complete tracing profile.

The old blanket failure wording in the pipeline/status trackers was stale:
one-second passes were already recorded in the preceding history-reader section.
Fresh pre-change `automation-schedules-5j6QCp` and measured `k2yW5v` also pass.
The measured baseline has eight committed claims, median 152625 us and maximum
243564 us, with no slow-checkpoint warning. This increment is a reduction of
repeated validation cost, not proof of the complete cause/fix for all earlier
contention/native-exit failures. Loaded and physical acceptance remain open.

Final locked/offline workspace/all-target suites pass **758 tests plus one
ignored each**, including no-dashboard: eef 247 plus ignored opt-in profiler,
owner CLI 17, installer 6, eefn 470, node CLI 18. Fresh headless binaries/examples,
formatting/diff, JS syntax and the backend/browser source guard pass.

Fresh final-binary `automation-schedules-mc895D` passes the unchanged ordinary
one-second/native/concurrent-activation, stop, revocation, restart, uncertain
enable/resolution replies, refusal and fresh-replacement cases. Its eight claims
commit with median 115649 us and maximum 197281 us, with no slow-checkpoint
warning or refused commit fence. The single pre/post comparison is a local
debug-build sample, not a controlled benchmark or guarantee under load. Both
validation phases still run; no window or permission was increased.

Final-binary `local-pipelines-6ldaE8` also passes finite dataflow, durable
progress, permission loss, native failure, recurrence and restart/no-replay
regressions. Both harnesses completed with no matching fixture eef/eefn/native
helper processes remaining. No installed/physical node, UI, model, release/feed,
signing or shared-service changes follow. Full stable 0.4.0 acceptance remains
incomplete.

Evaluated headless binary SHA-256:

```text
eef.exe  0e5ef5e0de39ce08b2548a3781418a04a81af8c1d08f85254501d97281934787
eefn.exe 5c5751288c87abf4ca20cee455a13ada0d98d6c806e801a8ec5dfb01ad8877e4
```

## Single-view timer observation (2026-10-23)

The timer now reads its current schedule and, only when needed, its associated
job from one freshly validated view under one journal lock. Previously it read
the schedule, released the lock, then independently revalidated history to find
the job. This removes that duplicate outer history read and lock acquisition;
it does not remove the job's own lineage/scope/retained-record checks.

Launching work still does not expose partially admitted jobs to the timer.
Non-running schedules do not cause periodic job inspection. Explicit owner
inspection continues to validate the retained job, including when the schedule
has stopped. Missing/unresolved work is not completion or permission to replace
an occurrence. Later claim, settlement and grant writes retain their independent
current-state, clock, authority and budget checks. There is no view cached across
calls/writes, new automatic retry, expiry/lease extension or schema change.

Slow snapshot warnings now distinguish `lock_wait_ms` from `validation_ms`, with
the existing 500-ms diagnostic threshold. Logging happens after releasing the
journal and includes no source, parameters, grant bodies or credentials.
The optional ignored `profile_read_only_timer_journal` test also measures full
retained-job validation and separate versus single-view timer observation on a
supplied read-only isolated fixture. It does not open/recover a live runtime or
issue work. Timings are local diagnostic samples, not acceptance thresholds.

The motivating package-doctor regression run stopped at a 45-second recurring
pipeline observation deadline. Its first occurrence completed; the second was
accepted/running at fixture shutdown, with no proved final outcome. That failed
attempt remains retained and is not replayed. A separately identified fresh
fixture passed unchanged before this timer edit, so intermittent latency is not
claimed fixed by this optimization. A read-only profile of the failed journal
found 27 retained jobs taking approximately 0.2 seconds to validate on that run;
this alone does not explain the earlier multi-second checkpoints.

The new regression checks fresh state after writes, read-only behavior, current
source validation, launch/stopped inspection boundaries and damaged claim refusal.
An initial compile check incorrectly hid a point reader still used by trigger
launch; that visibility was restored. An initial corrupt-job fixture erased its
lookup context, exercising missing work rather than malformed matching work;
the fixture now retains its context while corrupting the review. Both failed
checks remain in separate logs. The corrected focused regression and opt-in
read-only profile pass in
`.validation/package-doctor-scheduler-single-view-final-focused.log`. The three
single-view samples are 69,981 / 71,648 / 74,257 microseconds versus separate-read
samples of 78,100 / 78,839 / 80,074 microseconds. This modest local difference
does not explain or close the earlier intermittent multi-second stalls.
Both corrected locked/offline workspace/all-target feature modes now pass 1,045
tests plus one ignored each across fourteen target groups. Logs are
`.validation/package-doctor-timer-default-final.log` and
`.validation/package-doctor-timer-headless-final.log`. Headless apps/examples
also build. All twenty-four fresh native suites now pass in one sequential run,
including the unchanged scheduler and local-pipeline suites. Thirty-six
source/helper and nine rebuilt native hashes remain unchanged; all twenty-four
public fixture results report `passed: true`, and no matching fixture app/helper
processes remain observed. Final native log:
`.validation/package-doctor-timer-native-complete.log`; read-only inventory:
`.validation/package-doctor-timer-acceptance.json`.

Scheduler fixture `automation-schedules-GHpopG` still reports ten slow journal
checkpoints, maximum 874 ms; pipeline fixture `local-pipelines-7a2mKn` passes the
original recurring-work deadline and limits. These are local debug-build samples,
not a load/latency guarantee or proof of the earlier stall's cause/fix. The
retained pre-change timeout/unknown occurrence is not relabelled or replayed.
Physical two-PC/media, full onboarding/model quality, package/signing/Defender
and stable-release acceptance remain open. CI stays local; no installed node,
UI, model, version, release artifact or update-feed policy changes follow.
