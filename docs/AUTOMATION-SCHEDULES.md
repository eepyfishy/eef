# Owner-controlled automatic schedules (unreleased after alpha.6)

This connects the durable trigger/one-shot launch foundation to a bounded
runtime timer task and guarded owner commands. Fixed-input one-shot templates
can now create fresh ordinary requests, compile them, reserve declared authority
and execute through the existing reviewed grant driver. This is not general
semantic automation, a second runner, continuous media or stable 0.4.0.
Nothing activates merely by saving a definition/policy or starting EEF.

## Commands

First create/inspect an ordinary compiled source, save its disabled definition
and approve an exact bounded policy using the existing command families. Then:

```powershell
eef automation-schedules enable --policy POLICY_UUID --policy-sha256 POLICY_HASH --operation-id SCHEDULE_UUID --json
eef automation-schedules show SCHEDULE_UUID --json
eef automation-schedules list --limit 25 --json
eef automation-schedules stop SCHEDULE_UUID --revision CURRENT_REVISION --operation-id STOP_UUID --json
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

Backward wall time, wall/monotonic delta disagreement above 500 ms, or a polling
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
Partial-source/no-work closure, payload compaction and first-time friendly
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
