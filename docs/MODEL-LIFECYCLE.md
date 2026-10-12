# Selected GGUF lifecycle commands

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
include `unloaded`, `loading`, `ready`, `active`, `warm_idle` and `error`.
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
Unload retains the process/tree handle through a bounded root termination wait.
No user model/projector files are deleted. State/PID withdrawal is not a measured
physical RAM/VRAM release guarantee or proof that arbitrary external effects ended.

The implementation defaults to an eight-entry queue, 256 retained receipts
without eviction, 300-second load health readiness and a five-second root
termination wait. The subsequent [owner lifecycle policy](MODEL-LIFECYCLE-POLICY.md)
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
explicitly owned versus shared Ollama control, on-demand loading and broader
per-model policies, memory/VRAM measurement, resource-aware model-instance admission,
startup-cost routing, default interpreter quality/bootstrap and release packaging.
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
