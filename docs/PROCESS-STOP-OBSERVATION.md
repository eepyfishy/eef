# Owned-process stop observations (in development)

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
```

The first command remains the existing exact stop request, not a completion ACK.
The status commands need only the original job ID. The node command uses its
running authenticated connection and is restricted to that origin's job. The
coordinator command is local-owner-only. Neither starts a runtime, enables a
permission, submits work, sends/retries a stop, renews a lease or reconciles job
tracking. No UI polling/controller or model is involved.

Owner clients POST a strict versioned request with the expected current runtime
to `/api/commands/jobs/stop-status`. The backend captures the original revision
and stop-delivery targets, with at most two concurrent, five-second read-only
observations. Historical reads still require the same pinned issuer and node;
reconnect never authorizes a replacement grant or ordinary-RPC fallback.

## What is retained and displayed

A confirmed native observation is retained in the original normal execution
report, bound to its exact grant digest. The process action completed
unsuccessfully after the stop notification; its report uses `completed`,
`success:false` and `process_exit_unsuccessful`. Stop handling discards command
output, marks it truncated/discarded, and retains only bounded exit/observation
metadata. This is not successful work or authority to dispatch dependencies.

Status requires both original retained stop intent and the matching persisted
execution report. It returns minimal typed metadata, not prompts, program
arguments, stdout/stderr, model input or media. Its coverage counts explicitly
identify issued attempts not inspected; observing one tree never proves every
job/network effect stopped. The coordinator's tracking revision is a snapshot
and stays unchanged even if final node evidence now exists.

Late/unconfirmed cleanup, policy or clock loss, abandoned execution futures,
failed checkpoints and expired leases remain uncertain. Stopping before the
first operation poll cannot manufacture an observed tree. A later stop cannot
relabel an already-completed command or overwrite its result. Missing/corrupt
history is not proof no effects occurred and never permits automatic replay.

## Compatibility and remaining scope

Ordinary RPC does not receive the private cancellation input. Direct admitted
Windows processes alone use this observation path; Linux root-only execution,
finite pipelines, peer flows, shared model services, Python workers, media and
device buffers retain existing intent/unknown guarantees. Unsupported, offline
or changed-credential peers produce unconfirmed/unavailable status, never a
positive stop claim. No permission or transport exception is introduced.

The common capability dispatcher returns a boxed future before selecting a
provider. This keeps unused providers' state out of its callers' stack frames;
the resource, permission, lease and cancellation checks inside it are unchanged.
A portable regression constructs but does not poll the dispatch paths and
requires both direct dispatch and budget forwarding to retain that boundary.
The preceding native audio regression exited with a Windows stack-overflow
status. Its failed fixture is retained; the exact crash call site is unproven,
and this mitigation needs fresh native/full acceptance without larger stacks,
longer deadlines or weakened assertions.

Validation of this increment is pending. Native tests must independently hold
creation-verified root/descendant handles, observe actual exits, exercise journal
contention and verify exact retained reports/no replay. Full regression, hosted,
physical two-PC, continuous work, broader acknowledged stop/resource budgets,
model onboarding and installer/signing/Defender acceptance remain required.
