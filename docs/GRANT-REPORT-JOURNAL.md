# Owned grant-report journaling (unreleased after alpha.6)

The coordinator's node gateway now saves validated grant reports on bounded
blocking workers, without holding its shared pending-request mutex during the
observer's journal validation/write. This extends the existing reviewed-grant
path; it is not a second executor, a new transport or execution authority.

## Correlation and durable ordering

Acceptance still requires the exact authenticated node/session, issued grant,
one consumed lease proof and the unchanged coordinator monotonic deadline.
Before releasing the pending mutex, the gateway reserves this acknowledgement
with a fresh private checkpoint ID. Another ACK or completion cannot overtake
that save. After saving, it must find the same pending node/session/grant and
checkpoint ID before marking acknowledgement or emitting its informational
broadcast. Timeout, disconnect or replacement never restores a removed entry.

Completion/rejection keeps its existing grant/ACK-order checks, consumes the
correlation once and releases the shared mutex before saving. A normal RPC result
is delivered only after the direct observer succeeds. Broadcast listeners are
never a persistence mechanism. Observer failure, unavailable worker admission,
worker panic or shutdown returns uncertain journal failure, not no-effects proof,
automatic replay, fallback or a new lease.

## Ownership and bounded work

The internal safety ceiling permits at most 64 owned report checkpoints across
the gateway. Admission fails busy rather than adding an unbounded queue; this
limit does not authorize or reserve workload capacity. Ownership includes queued
and running blocking callbacks and successful saves awaiting async result
delivery. Cancelling an async waiter does not forget its save.

Stop closes admission, shuts down the owned session handlers and drains report
ownership before clearing remaining correlations. A queued callback starting
after closing skips the save. An already-started save must finish; late normal
confirmation is refused. Cancellation of Stop retains closed admissions and
callback ownership for the next Stop. Start cannot reopen report admission while
old saves/results remain owned. Storage stalls can still delay shutdown.

This ownership is necessary because started blocking work is not cancelled by
aborting its async waiter. See [Tokio's blocking-task contract](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html).
The drain observes the latest ownership count using a watch channel, following
[Tokio's watch receiver semantics](https://docs.rs/tokio/latest/tokio/sync/watch/index.html).
No dependency/version change is introduced; the locked workspace uses Tokio
1.53.1 and compiles against that local dependency.

## Verification and limits

The final focused server/module run passes 54 tests, including eight new cases:
held ACK and completion saves on a current-thread executor; independent request
correlation while storage is held; no premature completion or duplicate ACK;
disconnect/replacement fencing; cancelled acknowledgement and Stop waiters;
bounded admission; cancellation of a callback queued behind the sole blocking
thread; observer panic; and ownership through successful result delivery.
These cases also retain the original lease deadline and durable ACK/result order.
An intermediate test-only `unwrap_err` required an unnecessary Debug bound on
the private ownership token; explicit cancellation matching corrected it before
the final passing focused run. The compile failure is not counted as a pass.

Default and dashboard-free workspace/all-target suites each pass **623 tests**:
EEF library 237 plus one ignored opt-in profiler, owner CLI 16, installer 6,
node library 353 and node CLI 11. Fresh default binaries/examples, formatting,
diff and the backend/browser dependency guard pass; the full chain exits 0.

Fresh default debug binaries pass all three public fixtures, run separately
without a competing compiler:

- `automation-schedules-p34PzH`: three native occurrences, one concurrent
  activation, revocation/stop/restart, uncertain receipts, actual pre-launch
  refusal, explicit proof-bound resolution and fresh replacement.
- `local-pipelines-TDLPo0`: finite native/model dataflow, atomic child-provider
  accounting, permission loss, known failed native stage, two fresh bounded
  recurring parents and no replay/rearm after restart. The model is synthetic.
- `process-lifetime-O7Dtrp`: actual contained native process outcomes, including
  normal/nonzero exits, timeout, lease deadline, permission loss and restart,
  preserving uncertain outcomes and one attempt without replay.

The public verification chain exits 0. These are two authenticated logical
nodes on one PC, not physical two-PC, real-model-quality or loaded-system proof.
Fresh optimized workspace binaries/examples also build successfully offline.
The same unchanged public fixtures pass independently against `target/release`:
`automation-schedules-BxuBsY`, `local-pipelines-x2PaRJ` and
`process-lifetime-OXh0FP`, with complete release verification-chain exit 0.
Their results retain the same scope/exclusions; no installed app, installer,
bundle or versioned release asset is replaced by rebuilding these private targets.
Same-session frame processing still awaits its report save, and other command,
issuance and journal paths remain synchronous. Contention, loaded-system and
physical-network acceptance are not proven fixed by these focused tests.
Existing failed scheduling/recurrence evidence remains failed; isolated passes
do not establish their complete cause or a universal timing fix. This increment
does not complete strong remote stop, model bootstrap, continuous media, peer
transfers, distributed resources, cluster fencing, MSI/signing or stable 0.4.0.
No installed application/node, release/feed, dashboard or model is changed.
