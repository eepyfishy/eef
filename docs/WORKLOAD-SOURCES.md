# Continuous source inspection, planning and conditional approval

`eefn workloads sources` inspects the running local node. The coordinator owner
can inspect the same node with `eef node workload-sources --node NODE_ID`.
Both use backend operations; a dashboard or model is not required.

The current development path also supports independently inspected camera motion
sources through `eefn workloads camera-sources` and
`eef node camera-workload-sources --node NODE_ID`. Compilation, conditional approval
and explicit execution select the reviewed source kind; a camera request cannot
fall back to microphone inspection or preparation. Camera-capable peers advertise
an explicit version-three source list. Microphone-only peers retain the existing
version-two wire shape. Fresh readiness withdrawal updates the same authenticated
connection without restarting work. JSON-free resource setup and request
authoring through the public command lifecycle pass simulated-device hosted
verification at `cf07f93`. The generated-video decoder case also passes at
`7f1d052` under an explicit 512 MiB reviewed test grant. Neither result establishes
physical webcam/RTSP, default-memory, VLM-trigger or full release acceptance.
See [camera development and acceptance boundaries](CAMERA-MOTION.md).

The earlier implementation checkpoints below retain their original scope.

The later vision candidate adds `eefn workloads vision-sources` and
`eef node vision-workload-sources --node NODE_ID`. These are passive camera/model
configuration reads over the existing command services. They separate applied
bindings, owner approval and configuration readiness from installed-model or
physical readiness, without probing a device/provider or exposing private
endpoints. A later transport candidate independently negotiates vision execution
through a separate versioned advertisement and offer; it still needs hosted
verification and complete public job authoring/placement integration. This
inspection does not turn a camera-motion grant into model authority. See
[local event vision](LOCAL-EVENT-VISION.md) for verification and remaining work.

The next command candidate provides JSON-free vision authoring:

```text
eefn workloads vision-sources
eefn models installed
eefn requests submit-vision --camera CAMERA_ID --backend ollama --model MODEL_ID --model-digest INSTALLED_DIGEST --prompt "Describe activity."
```

Replace the selectors and digest with the source node's applied selection and
exact installed-model digest. For an applied llama.cpp vision selection use
`--backend llamacpp` and omit `--model-digest`. `--source-node NODE_ID` selects a
different disclosed source; the images and inference stay on that source node.
The command saves and compiles a request for review. Inspect its job/plan, then
use the existing separate owner approval and explicit execution steps. It never
enables a permission, selects/loads a model, contacts a model or starts capture.
After an uncertain reply, inspect the exact operation receipt before taking
another action. This candidate still needs complete hosted command/lifecycle
acceptance; it is not a stable feature declaration.

Inspection lists the applied microphone resources, their private-binding digests,
and the compiled-in energy adapter digest. It reports saved/applied microphone
permission, local listening consent/revision/epoch, whether saved resource
bindings still match the applied runtime, and whether Python is configured.
It does not import a device module, open a microphone, infer device availability,
choose a model, change preferences or authorize execution. Empty resources can
be prepared through the existing `eefn relay configure --microphone` command;
apply configuration with an explicit node restart.

Private device names/indices, adapter paths, resource parameters and properties
are omitted from the response. Unsupported private microphone bindings have no
usable digest. The same validator checks the actual local binding against a
reviewed definition. Device changes require a fresh review; a matching digest is
not a physical device identity or source ownership attestation.

Remote inspection uses the existing bounded control channel with independent
credentials and pinned TLS. The coordinator checks registration freshness,
runtime identity and the authenticated session before and after the read. The
node refuses this action over legacy shared-secret sessions. Older peers have
no fallback. The read-only operation uses reserved control capacity.

The existing explicit request compiler now accepts an exact continuous template
after immutable-origin/disclosure checks and a fresh authenticated source read.
The source/resource digests must match, and pending resource edits are refused.
Its checkpoint shares the existing task engine's shutdown gate. Retained retries
are read-only; compilation creates no approval, session offer or capture. The
node-facing planning view validates the exact nested source and task binding.
Legacy operation approval continues to refuse session plans.

After inspecting the retained plan, its owner can use
`eef requests approve-session ID --revision REVISION --digest PLAN_SHA256 --valid-for-ms MILLISECONDS --operation-id UUID`.
The distinct version-two operation saves approval for the exact source and all
reviewed limits through the same engine shutdown gate. It checks a fresh source
snapshot and independently authenticated pinned session before saving. A changed
source, pending resource binding, stale runtime or conflicting receipt is refused.
An exact retained retry is read-only, including offline or after shutdown, expiry
or cancellation; it cannot extend approval validity or modify the source.

Approval is conditional authority for later explicit admission. It does not turn
on listening, change microphone permission, install an interpreter, start a source
or issue a lease. Local consent may still be off at approval time. Admission must
check current local consent, permission epochs, source ownership and quotas before
any effects. The response explicitly reports no dispatch or execution authority.
Only the local coordinator-owner API exposes approval; node-origin planning has
no approval variant.

Session execution remains unavailable. The compiled-in adapter bytes provide
the intended source identity; actual execution still needs a separately admitted
owned source, current local permissions/listening epoch, renewable authority,
resource/process quotas, bounded durable events and observed termination. The
ordinary Python streaming bridge still opens plugin paths and retains its
existing duration limit; this change does not make that API a pinned workload
runtime or silently extend it.

The separate [pinned streaming counterpart](PYTHON-STREAMS.md) is in development.
It accepts owned, digest-checked trusted source bytes and avoids reopening a
mutable plugin path. It retains the ordinary bridge deadline and memory/output
limits. Wiring the compiled-in source to durable workload consumption, current
consent and leases remains required; the new primitive alone enables no capture.

The [prepared microphone backend](PREPARED-MICROPHONE.md) now composes those
pieces in a private node-owned source factory and adds shared Windows capture
ownership. It remains in hosted validation and has no public launch route.
