# Controlled node restart and owned-model shutdown (unreleased)

Extends the targeted [model unload contract](MODEL-LIFECYCLE.md). This is not a
stable release, physical second-PC acceptance or a general workload-stop claim.
The dashboard remains a client; existing owner-approved restart commands use
the same runtime implementation with no browser requirement.

## Replacement is gated on observation

The outer node supervisor requests shutdown through a scoped channel and awaits
the same running node future. It no longer drops that future on restart. Signal
handling has one owner; shutdown is considered before model-startup work when
both are ready. Model admission closes, pending lifecycle receipts become
interrupted, and no late load/inference pin can revive the retiring runtime.

Owned process/tree handles from cancelled startup, load or unload are retained in
the original model server. Cleanup requests termination without discarding those
handles. A later observation uses those exact objects, not PID-based termination,
endpoint disappearance or an empty resident-process map as exit proof.

Controlled stop uses the applied `termination_wait_seconds` as one absolute
budget for operation admission, concurrent group observation and root reaping.
The applied value is retained even after the old lifecycle runtime is dropped;
it is not replaced by newly saved preferences or a second cleanup allowance.
Windows observes each owned job's active-process count reach zero and reaps its
root. Other platforms retain root-only guarantees. Successful observation frees
the retained objects; cancellation, error or a late result retains ownership and
an unconfirmed fence. Group transitions cannot turn that uncertainty into success.

The parent retains an old-server shutdown barrier. A replacement runtime cannot
initialize through it until stop observation succeeds. An interactive node with
the owner API enabled keeps error/diagnostics available and waits for another
explicit owner restart; noninteractive failure returns an error. In either case
there is no automatic restart loop. That later intent checks the same retained
handles before initializing a replacement node/model runtime. Selected-version handoff is also
blocked while a barrier remains. It does not retry inference, replay work or
refresh an old execution grant. Missing handles cannot be invented as proof.

## Status and recovery

Pending group stop uses the existing precise `unloading` diagnostic and unavailable
accounting. Retired native objects are not falsely reported as zero usage or
`no_owned_job`. Failed unload remains `unload_unconfirmed` and cannot load again
within that runtime, including after a group transition or later observation.

An ordinary startup/load error can allow a new explicit owner load only after
the retired root is reaped and, on Windows, its owned job is observed empty.
Read-only status can collect that exit observation and release completed objects;
it does not load a model or continue a request. Live/unconfirmed retired objects
block replacement admission. Existing old registration readers retain the same
conservative busy/error projection and unavailable routing.

The restart command's existing `completed` still means a changed runtime ID, not
model readiness, coordinator reconnection or job recovery. It now follows the
controlled old-model cleanup barrier. A lost/negative/unconfirmed reply still
requires inspection; no adapter retry/fallback is added.

Abrupt process death, OS failure and forced external termination retain the
existing kill-on-close fallback; they are not newly claimed as observed shutdown.
No memory/VRAM-release measurement, rollback of external effects, shared Ollama
service termination, force-unload permission or continuous-pipeline stop is added.
Installed apps, remote physical nodes, UI, release assets/version and stable
update policy remain unchanged by this development increment.

## Verification

Focused contract checks cover pending/cancelled/failed/late group observation,
retained original handles, deliberate missing-evidence refusal, same-runtime
fencing, operation-admission deadlines, graceful supervision and cleanup-failure
propagation. Public tests use synthetic finite descendants with a separate native
observer holding creation-verified live identities before restart, both during
startup and with ready models. These do not replace physical/real-media or
default-interpreter acceptance. Final run IDs and complete-suite outcomes are
recorded below. All fixtures remain isolated from installed/physical nodes.

### Public and real-model evidence (2026-10-18)

Both locked/offline workspace/all-target suites pass **745 tests plus one ignored
each**, including no-dashboard. The default suite is rerun after the final atomic
ownership-transfer review. Fresh binaries/examples, formatting, JavaScript syntax,
backend/browser source boundary and diff checks pass. Validation processes are
terminal after cleanup. The six new unit tests and extended existing cases check
supervision, original-handle retention, lost-evidence refusal, stop admission,
ordinary-error replacement guards and serialized failure withdrawal.

`model-startup-r7pv8V` passes registration/control while loading, controlled
restart during startup, readiness without reconnect, unexpected-exit group
withdrawal, sibling cleanup and explicit recovery. Its native observer holds two
live, creation-verified nested-process handles before restart and confirms both
exit (`observed_after_ms:677`, since observer admission, not a shutdown benchmark).

`model-lifecycle-Jz7wAo` passes targeted unload, unchanged sibling, exact approval,
active pins, cancellation, historical no-replay and lost/malformed reply checks.
It additionally refreshes a ready synthetic model's finite descendants, holds
their live identities, then restarts the node and observes both exited
(`observed_after_ms:580`). `model-policy-zONquI` passes manual/eager/on-demand,
idle, saved/applied and owner-approval regressions. These are one-PC synthetic
model servers with real native process observation, not physical two-PC tests.

An intermediate `model-startup-I6Nwiu` run failed the required error-state group
withdrawal assertion. The failure is not recategorized as a pass. Final source
serializes monitor cleanup and restores error state after observed cleanup;
the fresh build passes the same assertion without changed expectations. The
startup tool now includes private failure-state diagnostics for future
investigation. Existing failed validation directories are not rewritten.

`real-model-HV3XFX` passes reviewed pinned-TLS CPU inference, controlled restart
with selected weights, explicit unload/reload, historical no-replay, inference
after reload, automatic idle unload and an on-demand load/inference/idle cycle.
It reuses the existing exact-size/hash-verified 1,280,835,840-byte artifact
`aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223`.
There is no model download/copy and no compiler runs during real inference.
`model_quality_accepted` and physical memory-release measurement remain false.

Evaluated node binary SHA-256:
`489f098bb0dbf6bd4d016b4826a97d271d3ccc3695a1399fa9a9132e4efcd697`;
coordinator:
`0c74f492773a0f7e1c652a97d953e23b2811d7cea243840cc4083e89426fde50`.
These identify the local evaluation builds, not reproducible-build attestation,
signed release assets or installed/remote-node updates.
