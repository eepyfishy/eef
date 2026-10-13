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
At this checkpoint, same-session frame processing still awaited its report save;
other command, issuance and journal paths remained synchronous. Contention, loaded-system and
physical-network acceptance are not proven fixed by these focused tests.
Existing failed scheduling/recurrence evidence remains failed; isolated passes
do not establish their complete cause or a universal timing fix. This increment
does not complete strong remote stop, model bootstrap, continuous media, peer
transfers, distributed resources, cluster fencing, MSI/signing or stable 0.4.0.
No installed application/node, release/feed, dashboard or model is changed.

## Framed-session responsiveness follow-up (unreleased)

The next source increment separates each session's whole frame-reading loop
from its FIFO report consumer. Both futures remain owned inside that session,
not detached tasks. A report save no longer prevents that same connection from
accepting heartbeats, submissions, fresh lease queries or correlated control
responses. Workload ACKs and completions stay in FIFO order and pass the existing
durable acceptance/authority checks at processing, never from a queued snapshot.

Intake requires an exact pending authenticated node/session/grant and a valid
matching report kind/success flag. It is only queue admission, not an ACK or
execution permission. Unknown, substituted or malformed reports are not queued.
Each session holds at most 64 queued envelopes; the gateway shares a 32 MiB
serialized-envelope allowance across sessions, including active envelopes until
processing ends. Size is counted with a bounded writer without allocating another
serialized payload. The message moves into processing rather than being cloned.
These bounds are not measured total RAM/CPU/VRAM or physical resource guarantees.
Cancelled consumers release envelope allowances; started journal callbacks remain
independently owned by the report-worker manager until save/result disposal.

On queue pressure or malformed framing, the session retires and its pending
callers retain uncertainty, rather than silently dropping evidence and reporting
success. Orderly EOF drains already-received reports in the original wire order;
gateway shutdown still cancels owned sessions and drains started report callbacks.
No new retry, fallback, expiry allowance or lease renewal is introduced. Queue
residence uses the original deadline; processing can still refuse an expired ACK.

The reader's pending `read_message` is never selected/cancelled for individual
report completions: selecting whole loops preserves partially read encrypted
frames. A held-save framed test receives a heartbeat and control reply, then
finishes ACK/completion storage halfway through the next encrypted heartbeat,
which must remain intact. Additional cases verify malformed-frame and byte-
pressure retirement, discarded successful save tokens after consumer cancellation,
exact intake identity, FIFO, byte/count limits and permit recovery. The inner
duplex tests use a known authenticated identity; existing gateway/public tests
separately cover real credential/TLS establishment.

Final focused server tests pass 62 cases, including eight new queue/framed
cases. Default and dashboard-free workspace/all-target suites each pass **631
tests**: EEF library 237 plus one ignored opt-in profiler, owner CLI 16,
installer 6, node library 361 and node CLI 11. Fresh default executables/examples,
formatting/diff and the backend/browser source guard pass.

Fresh debug public runs `automation-schedules-agcSrG`,
`local-pipelines-xtVgqX` and `process-lifetime-6U0jVG` all pass with chain exit 0.
Their native/recurrence/restart/refusal scopes remain the same as the earlier
report-worker fixtures above: authenticated logical nodes on one PC, no physical
network, real-model-quality or loaded-system claim. Optimized workspace binaries
and the native fixture helper also build successfully offline. Their public
ordinary scheduling (`automation-schedules-S888rF`) and native lifetime
(`process-lifetime-AgxCuj`) checks pass, but the pipeline test runner exits
`0xc0000409` in `local-pipelines-Gob8R6`, making that release verification chain
exit 1. No normal failure/exit report is emitted. Read-only inspection finds
zero retained jobs/schedules and no remaining fixture processes; Application
events inspected do not establish a cause. The failed DB remains unchanged at
SHA-256 `fc5c37e063f4b81c8a306f061c5acb22fd8f097995ac0d7673dcfb89af35b87b`.
No timing allowance or historical failed evidence is changed.

Other command/issuance journal paths, storage and
CPU contention, physical network/load acceptance and the complete stable 0.4.0
scope remain open. No installed node or published artifact changes follow.

### Pipeline runner diagnostics

The pipeline harness now appends bounded private phase checkpoints as phases
happen, because a native fail-fast exit may never call a JavaScript exit handler.
It also enables fatal diagnostic reports in its isolated fixture directory,
excluding environment and network details. No model/credential inputs or child
arguments are included in phase records; only phase/time/runner/binary-directory
and the last sixteen child process identities/statuses. Fatal-report handlers
are diagnostic opportunities, not guarantees for every native failure.

The installed runner is signed OpenJS Node.js 24.15.0. Upstream's
[Windows libuv fix](https://github.com/nodejs/node/pull/61999) is included in
[24.20.0](https://nodejs.org/en/blog/release/v24.20.0). It is a relevant hypothesis,
not a diagnosis from the exit code alone: our failed run has no native stack or
matching assertion. A private standalone 24.20.0 runner was downloaded from
the official release, verified against its published SHA-256
`5c976096e04e5c2c1f091938926234cc9fbebfe9787ddd149351b3b0ecc707b5` and a valid
OpenJS publisher signature. It adds about 89 MiB, without npm/archive copies,
global installation/PATH changes, application bundling or a model download.

The instrumented release pipeline run `local-pipelines-nyw8eS` passes on that
portable runner; its private phase trace reaches `passed` and identifies 24.20.0.
The original native failure remains failed evidence. This changed-runner pass
does not prove the complete crash cause, clear older native exits or guarantee
every Windows runner is safe. These are development tools, not a Node.js runtime
dependency added to EEF/EEFN. Physical/load/security and full stable acceptance
remain incomplete.
