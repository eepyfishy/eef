# Owned-process stop observations (development checkpoint)

An accepted Windows `process.exec` grant can now keep driving bounded native
cleanup after its exact durable stop request. It retains the owned unnamed job
handle while cancelling pipe/root waits, requests termination of that job only,
and observes zero active processes before its original intersected deadline.
It does not terminate by PID, process name or application path.

This is a scoped observation, not a sandbox, rollback, global job-stop guarantee,
hard real-time promise or termination of outside services/devices. Completed
actions are not undone. The separate v1 stop receipt and advertisement still mean
`durable_intent_only`; their execution/external-effects booleans remain false.

## Commands and backend ownership

```text
eefn jobs stop JOB_ID
eefn jobs stop-status JOB_ID
eef requests stop-status JOB_ID
eefn grants stop-status --id GRANT_UUID
```

The first command remains the existing exact stop request, not a completion ACK.
The two job-status commands need only the original job ID. The node command uses its
running authenticated connection and is restricted to that origin's job. The
coordinator command is local-owner-only. Neither starts a runtime, enables a
permission, submits work, sends/retries a stop, renews a lease or reconciles job
tracking. No UI polling/controller or model is involved.

Run `eefn grants stop-status --id GRANT_UUID` on the executor to inspect a
continuous workload while either service is offline. The command opens only
the node's existing journal in read-only mode and checks its owner and exact
grant scope. Add `--json` for the stored observation and original stop time.
Missing, corrupt or inaccessible evidence cannot confirm process exit.

Owner clients POST a strict versioned request with the expected current runtime
to `/api/commands/jobs/stop-status`. The backend captures the original revision
and stop-delivery targets on the bounded journal reader. The local snapshot and
up to two parallel executor observations share five seconds. If the snapshot
cannot be read, the command reports an error. Late or unavailable executor
evidence is marked unavailable. Historical reads still require the same pinned
issuer and node. The command uses the original grant scope after reconnect.

Source `5976d3d` passed all four jobs in
[run 44436626483](https://github.com/eepyfishy/eef/actions/runs/44436626483).
Windows/headless passed 1,884 Rust tests across 18 groups with one ignored,
all 37 native suites and the retained additional gates. The new held-journal
case checks the bounded read through the public stop-status command service.

## What is retained and displayed

A confirmed native observation is retained in the original normal execution
report, bound to its exact grant digest. The process action completed
unsuccessfully after the stop notification; its report uses `completed`,
`success:false` and `process_exit_unsuccessful`. Stop handling discards command
output, marks it truncated/discarded, and retains only bounded exit/observation
metadata. This is not successful work or authority to dispatch dependencies.

For a one-shot grant, status requires both original retained stop intent and the
matching persisted execution report. Status returns only typed process metadata.
Its coverage counts explicitly
identify issued attempts not inspected; observing one tree never proves every
job/network effect stopped. The coordinator's tracking revision is a snapshot
and stays unchanged even if final node evidence now exists.

For a continuous workload, status reads the node's separately retained owned
capture exit evidence for the original stop target. The node must advertise the
read-only `workload_stop_exit_read` control. The response has the original scope,
optional recorded stop time and optional process observation. A matching pinned
issuer can inspect it after reconnect. Older nodes count as issued attempts
outside this command's coverage. Use [stop-receipt](STOP-INSPECTION.md) for the
separate intent record. A process observation leaves coordinator tracking and
the interrupted workload's unknown outcome unchanged.

Late/unconfirmed cleanup, policy or clock loss, abandoned execution futures,
failed checkpoints and expired leases remain uncertain. Stopping before the
first operation poll cannot manufacture an observed tree. A later stop cannot
relabel an already-completed command or overwrite its result. Missing/corrupt
history is not proof no effects occurred and never permits automatic replay.

## Compatibility and remaining scope

Ordinary RPC does not receive the private cancellation input. Linux root-only
execution, finite pipelines, peer flows, shared model services, and device
buffers retain their existing intent and uncertainty behavior. Unsupported,
offline, or changed-credential peers produce unconfirmed or unavailable status.

### One-shot camera and microphone capture (development)

Windows nodes can also observe termination of the owned Python process tree for
an admitted `camera.capture` or `audio.capture` grant. On stop, the node closes
the exchange pipes and uses the original deadline to observe the root and its
descendants exit. It releases the capture reservation after confirming that
exact job's exit. A stop before launch produces no process evidence. If exit
cannot be confirmed, the existing uncertainty and cleanup paths apply.

The existing grant result carries `capture_owned_processes_stopped` with the
owned tree observation and no capture output. This is an unsuccessful capture
result. The observer requires the exact stop intent and retained report. A
matching JSON field from a plugin cannot create that result. The observation
covers the owned processes. Device buffers and external effects remain
unconfirmed. The original stop-intent receipt remains unchanged.

Source `a60e934` passed all four jobs in
[run 44437458182](https://github.com/eepyfishy/eef/actions/runs/44437458182).
Windows/headless passed 1,886 Rust tests across 18 groups with one ignored,
all 37 native suites and the retained additional gates. The node fixture uses
synthetic camera and microphone plugins, holds real root and child process
handles, and confirms that the capture reservation is reusable after exit.

The public command follow-up `1528fcf` passed the same counts and all four jobs in
[run 44438746327](https://github.com/eepyfishy/eef/actions/runs/44438746327).
Two separately paired logical nodes check `jobs stop`, matching read-only origin
and owner status, denied inspection by the executor as another origin, and
unchanged journal bytes on reads. The fixture observes both owned descendants
and checks that the retained report carries only the scoped process observation.
Physical devices, external effects and broader continuous stop remain open.

The common capability dispatcher returns a boxed future before selecting a
provider. This keeps unused providers' state out of its callers' stack frames;
the resource, permission, lease and cancellation checks inside it are unchanged.
A portable regression constructs but does not poll the dispatch paths and
requires both direct dispatch and budget forwarding to retain that boundary;
their caller storage falls from 19,472 to 16 bytes on the tested x64 host.
The preceding native audio regression exited with a Windows stack-overflow
status. Its failed fixture is retained; the exact crash call site is unproven,
and the failed fixture remains retained. Fresh native/full acceptance uses no
larger stacks, longer deadlines or weakened assertions.

Checkpoint `dd6ae21` passes both 1,217-test local modes plus one existing ignored
test each, 38 fresh local native cases, separate cached CPU/browser regressions
and [all four hosted jobs](https://github.com/eepyfishy/eef/actions/runs/40556221544),
including 32 Windows native suites and the separate connection-close case.
Creation-verified root/descendant exit observers, held-journal cleanup, exact
retained grant/stop/result reconciliation and read-only database hashes pass.
The new native process fixture retains two scoped stop observations among nine
original jobs/receipts; it never upgrades unknown outcomes or v1 intent flags.
Physical two-PC, continuous work, broader acknowledged stop/resource budgets,
model onboarding and installer/signing/Defender acceptance remain required.
