# Selected GGUF lifecycle commands

The subsequent [shared-Ollama preparation](OLLAMA-PREPARATION.md) operation
extends this same worker and receipt service. Its observed shared residency is
not the owned-process load/unload behavior described below.

Unreleased after `0.4.0-alpha.6`. This is the manual owned-GGUF increment,
not completion of the full model lifecycle roadmap or stable 0.4.0.
Commands use the running node service; no dashboard, model inference, model
download, selection change or node restart is required to control residency.
EEF itself still requires no local model.

## Commands

First inspect current state:

```text
eefn --json model-lifecycle show
eef --json node model-lifecycle --node NODE_ID show
```

Use the exact `result.runtime_id`, model ID and generation from that report.
Generate and retain a new UUID for each distinct load/unload intent:

```text
eefn --json model-lifecycle --runtime RUNTIME_UUID unload --model MODEL_ID --generation GENERATION --operation-id OPERATION_UUID
eefn --json model-lifecycle --runtime RUNTIME_UUID load --model MODEL_ID --generation GENERATION --operation-id OPERATION_UUID
eefn --json model-lifecycle show --operation-id OPERATION_UUID
eefn --json model-lifecycle --runtime RUNTIME_UUID cancel --operation-id LOAD_OPERATION_UUID
```

The EEF equivalents use `eef node model-lifecycle --node NODE_ID`, with the
same runtime, generation and operation arguments. Remote mutations require
the node owner's existing management approval; EEF cannot enable it for itself.
Status does not require mutation approval. Local commands require the running
node and its guarded loopback command service; there is no offline mutation.

An accepted command is queued, not necessarily completed. Inspect its exact
receipt until terminal: `completed`, `failed`, `cancelled` or `interrupted`.
`queued`, `running` and `cancel_requested` are nonterminal. A CLI wrapper's
`success:true` confirms the command report, not completion of the operation.
After a lost, negative or malformed reply, inspect the original operation ID.
Neither adapter retries, changes provider or fabricates a replacement intent.

Exact command/operation reuse is historical inspection, including after the
model has been reloaded. Conflicting reuse fails. Restart creates a new runtime
and invalidates old runtime mutations; never refresh that fence implicitly.
Receipts are runtime-local, not a durable model-operation journal. They are
not retained across restart or evidence authorizing workload recovery.

## State and lifetime

Each selected slot reports `model_id`, `state`, `generation`, `active_requests`,
`control_pending` and `owned_pid`. Private artifact paths are omitted. States
include `unloaded`, `loading`, `ready`, `active`, `warm_idle`, `error`,
`unloading` and `unload_unconfirmed`.
An inference pin changes ready/warm-idle to active; the last release publishes
warm-idle. Pending control withdraws availability before worker application,
so new inference cannot race a reserved unload. Active pins reject unload;
there is no force option and cancellation never stops inference.

Load starts only an already-applied selection. It checks current settings and
owner identity/authority under the configuration lock at native process creation, and
checks exact load ownership immediately before spawning. Unload obtains its
current-authority guard after waiting for operation admission and holds it
through ownership withdrawal. Saving changed model settings requires an
explicit restart before new lifecycle mutations; receipt inspection remains
read-only. Normal startup still starts the configured group as before.

Targeted unload/reload and failed/cancelled partial load affect only that slot.
Existing post-ready unexpected-process-exit monitoring still withdraws the
whole group and cleans surviving siblings; it never automatically restarts them.
Closing the runtime permanently closes the retained model server, interrupts
pending receipts and prevents stale inference pins or late loads reviving it.

Windows GGUF children now use the existing typed, non-inheritable, unnamed
job-object containment: suspended creation, assignment before resume, and
kill-on-close descendants. Failure has no uncontained/elevated fallback.
Non-Windows retains root-child cleanup only; no descendant guarantee is claimed.
Explicit and idle unload now retain the process/tree handle through one bounded
termination observation. On Windows, this terminates only the owned unnamed job,
observes its active-process count reach zero, and reaps the root before completion.
Non-Windows observes root termination only; no descendant guarantee is added.
No user model/projector files are deleted. State/PID withdrawal is not a measured
physical RAM/VRAM release guarantee or proof that arbitrary external effects ended.

### Targeted unload observations (unreleased)

Withdrawal advances the slot generation and retains its exact control reservation
while state is `unloading`. It is unavailable to inference, on-demand loading,
idle selection and replacement controls. Its job belongs to the termination
future rather than the resident map: status reports unavailable accounting, not
invented zero usage or `no_owned_job`. An empty process map is not exit proof.

Only successful observation before the same absolute deadline publishes
`unloaded` and permits a completed unload receipt. The existing
`termination_wait_seconds` covers both OS tree observation and root reaping;
there is no second cleanup allowance. A late ready result, observation error,
missing owned process, timeout or dropped unload future instead leaves
`unload_unconfirmed`. No inference or same-runtime load/startup retry is allowed
from that state. Inspect the original receipt and deliberately restart the node
before trying a new load. This requirement does not claim restart proves physical
memory release or that unknown external effects are reversible.

The terminal transition checks runtime, generation, reservation and pending state
again. Closing/replacing the runtime or another group-state transition cannot be
overwritten by a late stop result. A denied authorization before withdrawal leaves
residency unchanged. Sibling slots and shared Ollama services are not targeted.
The subsequent [controlled shutdown extension](MODEL-SHUTDOWN.md) retains
cancelled cleanup handles and gates normal runtime replacement on observation.
Abrupt external process death still uses kill-on-close fallback, not observed
shutdown. This does not extend the guarantee to arbitrary external effects.

Owner-command status carries the two precise new states. Registration preserves
the existing vocabulary for older coordinators: pending unload projects to
`loading`/`unavailable`, and unconfirmed unload to `error`/`unavailable`. This is a
legacy busy/error view, not a claim that another load is running. Updated model
metadata readers also reject routing for the precise new states when supplied.
Old owner CLI decoders may reject a new diagnostic enum; use the matching CLI,
not automatic fallback/retry. New clients still accept old reports and optional
accounting omission. No schema/grant permission, force-unload or replay policy is
relaxed.

### Targeted termination verification (2026-10-18)

Both locked/offline workspace/all-target suites pass **739 tests plus one ignored
each**, including no-dashboard. The final default suite is rerun after the
group-state fence refinement. New contract tests hold termination pending to
inspect non-routability/accounting, and cover observation errors, timeouts,
external future cancellation, missing ownership, non-yielding late completion,
group-state overwrites and closed-runtime late results. Same-runtime reload is
refused after every unconfirmed path. Typed-report tests accept the new exact
states and reject contradictory reservation/PID/accounting claims while retaining
old report compatibility.

Public `model-lifecycle-bMBUGH` passes the pinned-TLS owner command path, exact
remote approval, targeted unload/reload, unchanged sibling, historical no-replay,
active inference pins, partial-load cancellation and malformed/lost reply checks.
Its Windows model job contains the synthetic backend plus two nested finite
processes. A separate native observer holds both descendants' live,
creation-verified handles before unload and confirms both exited afterward
(`observed_after_ms:71`, measured since observer admission, not a termination
latency benchmark). This supplements the production job-empty observation and
pending-state ordering tests; it is one-PC synthetic process evidence, not model
quality or physical second-PC acceptance. Test helpers are never packaged.

`model-startup-L3izuZ` and `model-policy-9E2QBk` pass nonblocking startup,
failure withdrawal, restart, saved/applied policy, active-use pinning and
automatic idle/on-demand regressions. `real-model-zSuIyJ` additionally passes
reviewed pinned-TLS CPU inference, explicit unload/reload without node restart,
historical unload no-replay, inference after reload, automatic idle unload and a
later on-demand load/inference/idle cycle. It reuses the existing exact-size/hash
verified 1,280,835,840-byte artifact
`aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223`
without a model download/copy; interpreter-quality acceptance remains false.
No compiler runs during that real-inference fixture.

Fresh default binaries/examples build; formatting, JavaScript syntax,
backend/browser source-boundary and diff checks pass. The evaluated node binary
SHA-256 is `3ca5b6d8b767dba6d7ac0829d5b0829605a5798ca0b07ff424b3f2d05233511a`;
coordinator is `d9af1eac72be8d0032d58079302f293794d4f094cd7b7c6e4b81939b5fb1851a`.
Validation processes are terminal after cleanup; no model files are deleted.
No installed/physical node, shared Ollama, UI, release version/assets or stable
feed is changed. Physical RAM/VRAM release, ordinary group-stop observation,
default interpreter/bootstrap and full release acceptance remain open.

The following [controlled shutdown increment](MODEL-SHUTDOWN.md) supersedes this
checkpoint's ordinary group-stop observation gap for controlled model-runtime
replacement only. Abrupt death, arbitrary effects and broader workload/resource
acceptance remain separate.

The implementation defaults to an eight-entry queue, 256 retained receipts
without eviction, 300-second load health readiness and a five-second owned
termination observation. The subsequent [owner lifecycle policy](MODEL-LIFECYCLE-POLICY.md)
configures these bounds, startup mode and optional monotonic idle unloading.
Queue rejection releases
only the rejected reservation; receipt exhaustion refuses new admission rather
than discarding replay protection. Status and exact load cancellation use existing
bounded reserved network control capacity; load/unload use workload admission.
Reserved action names reject substituted mutating commands.

## Service contract and acceptance

Guarded node endpoint: `POST /api/commands/model-lifecycle`.
Guarded EEF-owner endpoint: `POST /api/commands/node/model-lifecycle`.
Both accept the shared schema-1 `LifecycleRequest` with `expected_node_id`,
optional `expected_runtime_id` (required for mutation) and tagged `command`.
Remote dispatch uses the existing authenticated `node.configure` control path,
not a new listener. Strict confirmation correlates node/runtime/command/receipt
and rejects extra fields or inferred authority. Reports always declare no
inference authorization, file deletion or automatic retry.

`node tools/test-model-lifecycle.mjs` exercises public commands, pinned TLS,
owner approval, slot isolation, historical no-replay, active inference pins,
reserved status during work saturation, partial-load cancellation, Origin
rejection, stale-runtime refusal and lost/corrupt/negative/unsupported local
replies without retry or fallback. It uses two synthetic model servers on one
Windows PC, not actual weights, model quality or physical second-PC acceptance.
The synthetic helper is test-only and excluded from release payloads.

`tools/test-real-model.mjs` additionally checks the same commands with opt-in
verified real CPU weights: warm-idle, targeted unload/reload without node restart,
historical no-replay and subsequent reviewed inference. It can reuse an existing
exact-hash artifact without copying or downloading it. Passing that test is not
interpreter-quality, physical RAM/VRAM-release or second-PC acceptance.

Remaining full-release work includes physical-network lifecycle acceptance,
explicitly owned versus shared Ollama control, broader per-model policies,
memory/VRAM measurement, resource-aware model-instance admission,
startup-cost routing, default interpreter quality/bootstrap and release packaging.
Optional owned-GGUF on-demand loading is implemented in the subsequent
[lifecycle policy increment](MODEL-LIFECYCLE-POLICY.md); it does not complete those gates.
Selecting Ollama currently reports owned-GGUF lifecycle unsupported and refuses
these mutations; it does not kill the user's Ollama service or switch backend.

## 2026-10-12 verification checkpoint

Final workspace/all-target suites pass **545 tests** in each feature mode:
EEF library 219, owner CLI 14, installer 6, EEFN library 297, node CLI 9.
Fresh default binaries/examples and formatting/diff/JavaScript/backend-boundary
checks pass separately. Unit checks include exact duplicate admission, queue and
receipt exhaustion without eviction, reserved-action substitution rejection,
active-use pins, closed-runtime/withdrawn-load fences and current application
authority after waiting for operation admission.

Final public `model-lifecycle-ENeBVY` passes the synthetic pinned-TLS command
path, including queued unload refusal after owner approval is revoked, local
owner cancellation while remote cancellation is denied, and a separate fresh
owner-approved load cancelled through EEF. Startup regression
`model-startup-0DctmF` passes the existing group behavior and cleanup.

`real-model-fi7H33` passes with the existing exact-size/hash-verified 1.5B
artifact, CPU workers set to two and explicit GPU-layer zero arguments. It
confirms targeted unload/reload without node restart, historical no-replay,
subsequent reviewed inference, preserved completed results across node restart,
and later selection withdrawal. Downloads and physical memory-release
measurement are explicitly false. The candidate remains unapproved as the
default request interpreter; a greeting is not an agentic-quality evaluation.

Earlier full-suite failure exposed a test's injected approval time preceding
actual source compilation under parallel load. The fixture now respects that
existing ordering; production evidence validation is unchanged. An initial
identity-replacement test attempted a normal owner save, which correctly
preserves stable identity. The corrected fixture uses an external file
replacement. Failed and intermediate runs are not final acceptance evidence.
No installed runtime, release/feed, physical node, UI or signing/Defender
certification is changed by this source checkpoint. Stable 0.4.0 remains open.

Final-source regression fixtures also pass: model metadata/selection
`model-metadata-MQp2Wf`, native process lifetime `process-lifetime-6i0cQR`,
automatic scheduling/refusal receipts `automation-schedules-Nj7kAS`, node input
and answers `request-input-UP4teB`, and reviewed execution/recovery
`request-review-WPdrnI`. These isolated fixtures are not physical-network,
general interpreter-quality or stable-release acceptance.
