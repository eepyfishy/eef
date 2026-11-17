# Event-triggered local vision in development

The v0.4 roadmap requires one camera acquisition feeding lightweight motion
detection, optional owner-approved VLM processing and bounded useful events.
The existing motion-only workload discards images and grants no model or image
transfer authority. A second `camera.capture` also conflicts with its active
camera reservation. This feature must extend the admitted source lifecycle;
repeating an unrelated finite graph is not continuous integration.

## Private source and pipe component

`python/streams/camera_vision.py` is a separate extension appended to the exact
motion helper bytes. It reuses validation helpers without changing the existing
motion adapter or its digest. It is lazy, single-open and retains the motion
baseline, dimension uncertainty and hysteresis. Explicit image settings bound
JPEG dimensions, quality, bytes, count, minimum interval and either rising-edge
or active-interval triggers. Unknown/quiet samples carry no image; image budget
or interval skips are explicit. Capture/encoding errors fail rather than inventing
an analysis or quiet frame. Native workers are limited before library import.

`python/eef_vision_stream_host.py` is a distinct private pipe protocol. It accepts
bounded, hash-pinned source bytes and requires an exact start message before
advancing the source, then an acknowledgement for each observation. The iterator
starts a new sampling interval after its consumer resumes it, preventing buffered
catch-up when processing is slow. The host independently validates observation
positions, image encoding/count/bytes, frame/aggregate output budgets and exact
completion. Duplicate fields, unknown fields, mismatched controls and early end
fail closed with a fixed error, without exception text in protocol output.

This component does not mint authority. Its caller must supervise the original
process deadline, resource reservation and reviewed image/model scope. It grants
no OS isolation of trusted Python code. It is not the legacy metadata protocol.

## Owned Rust transport under verification

The Rust private reader uses a separate strict frame type, validates sequence,
image and aggregate byte budgets, and sends start/ack controls only after their
corresponding checks and consumer completion. Images have a local, non-serializable
type whose debug output contains only their size. The reader requires exact end
counts and EOF; a plain summary cannot construct its owned-exit result.

Metadata and private image streams share the existing process ownership and
cleanup implementation. It retains the original absolute deadline, configured
job memory ceiling and camera reservation. Successful private completion requires
an observed empty owned Windows job; unsupported platforms refuse before spawn.
Consumer failure, deadline and malformed data retire the same owned process tree.
This transport supplies lifecycle evidence, not admission or model authority.

## Local model adapter under verification

The private engine adapter binds an explicit already-selected vision-capable
model and backend. It reuses the existing Ollama/llama.cpp payload builders,
response normalization, local operation-policy checks and llama.cpp model-use
pins. It accepts only the applied literal loopback endpoint and uses a separate
HTTP client with proxies and redirects disabled. It never accepts a caller URL,
chooses a substitute model, downloads a model, or invokes generated tools.
The locked HTTP client's default protocol retry policy is explicitly disabled;
absence of an application retry loop alone would not establish this behavior.

Ollama calls require the reviewed installed-model digest, one matching nonempty
installed-model report and no reported remote model/host, checked before and after
inference. These are reports from the trusted local service, not independently
measured model files or proof about that service's own external communications.
The llama.cpp binding retains the existing applied slot and owned-use pin; it
does not add a new file-integrity or hardware quota claim.

Calls have explicit token, response byte, result byte and time bounds. Current
authority and local policy are checked around effects and completion; incomplete,
oversized, changed-identity or failed calls expose neither partial text nor
provider diagnostics. Calls are never retried by this helper. Its caller must
persist the analysis intent first, supervise revocation/stop throughout the call,
and leave uncertainty on interruption. Cancelling HTTP does not prove an external
shared model stopped. Ten added tests use synthetic HTTP and the existing owned
model helper, covering both payloads, identity changes, limits, revocation,
expiry, and refused image-post redirects. Hosted verification is pending.

An additional Windows native fixture composes the actual pinned camera vision
iterator, private Python host, Rust owned transport and local model adapter.
Its fake camera/encoder and synthetic HTTP provider require one acquisition,
one image from the triggering frame, no next read while the model reply is held,
and camera reuse after owned exit. This is transport composition only; it does
not exercise workload admission, durable analysis decisions or a physical model.

## Private admitted runtime under verification

Vision analysis has a separate owner opt-in, defaulting to false and omitted
from legacy policy serialization. Ordinary workload enablement or camera
permission does not enable it. Disabling workload sharing also clears vision
approval; an inactive policy cannot stage a vision approval for later use. The
existing runtime/revision/epoch checks protect both local and remotely authorized
policy edits. The permission commands do not start capture or select a model:

```text
eefn workload-policy vision --enabled true
eef node workload-policy --node NODE_ID vision --enabled true
```

Workload sharing must already be enabled. `--enabled false` revokes this feature.
The complete vision job command path is still in development.

The private prepared driver uses the existing camera pool, consumed ACK, lease
supervisor and bounded native journal worker. Each image waits for a committed
intent, a fresh current-authority check, one selected local model call and its
durable result before capture can proceed. Metadata callbacks also follow durable
event storage. Quiet input opens no model connection. Permission, runtime,
resource, Python or model-configuration changes retire the original source;
restoring the old configuration cannot revive it. External shared model service
termination remains unproven when an HTTP call is cancelled.

Version-three completion joins the opaque owned source exit/image count, exact
filtered metadata prefix and the durable completed-analysis prefix. An unfinished
analysis cannot become successful completion, and historical results cannot
recreate execution authority. Optional analysis fields remain absent in old
completion reports. Four policy regressions and five explicit native fixtures
cover the new path, including held model replies, actual root/child process
handles, incomplete output, quiet input, revocation and denied preparation.
Media, encoding and model responses are synthetic; hosted verification is pending.

The initial admitted-runtime run `43989699329` passes both Linux jobs and
Windows/dashboard, but Windows/headless fails to compile two native fixture calls
to a test helper with sibling-inaccessible visibility. Its later independent
native diagnostics also observe exit 101 from the quiet node service, without
capturing the reason. That run is not accepted. The correction widens only the
test-only helper's visibility and adds failure-only startup diagnostics; an
observed exit or redirected diagnostic launch can never satisfy the quiet-service
acceptance gate. The startup cause remains unresolved pending a new hosted run.

The reviewed journal revision adds a distinct version-three `camera_vision` definition. It
binds camera and image settings, selected model/backend/digest, prompt, token,
encoded response/result and per-call time limits. Each image permits at most one
call; aggregate requested output-token and result reservations derive from the
reviewed image count. The Python request receives capture/image settings only.
Old metadata definitions retain version two, their fields and grant hashes.

Analysis persistence uses version six of the existing grant journal, with exact
schema checks, grant-bound intent/result records, counters and a checksum prefix.
A single-use acknowledgment follows the committed intent; a pending intent
prevents duplicate or later analysis. Results require that acknowledgment and
current reporting authority. No image bytes enter the journal. Combined metadata
and JSON-encoded result reservations must fit owner limits, with page/row overhead
reserved before consumption. The existing bounded native worker retains its slot
until actual completion, including abandoned or expired writes. Historical
inspection is issuer-bound and cannot recreate an acknowledgment or execution.
Six definition and six real-journal regressions cover binding, old-format
preservation, durable order, interrupted history, quotas, corruption/deletion,
downgrade and actual worker retirement. The journal revision `ac1717c` passes all
four jobs in [run 43906598633](https://github.com/eepyfishy/eef/actions/runs/43906598633),
including 1,804 Windows/headless Rust tests, one ignored across 17 groups, and the
retained native and command gates. This does not verify the later private runtime.

These definitions and journal helpers are not yet an executable network feature.
Metadata-only offers, source inspection and execution advertisements still refuse
the new kind. The private admitted driver and joined completion are now under
verification, with explicit owner opt-in. Compatible network negotiation, source
inspection, placement and public job/result commands remain required before
enabling the network workflow.

Eleven source tests use deterministic camera/image/clock fakes. Ten private pipe
cases test start/ack gating, slow consumption, image transfer, legacy EOF before
start, wrong acknowledgements, oversized/duplicate controls, source hash/error,
early end and unsolicited images. The existing required native source suite
invokes them after its original microphone/motion checks. Local JavaScript syntax
and diff checks pass. Exact source `cd7ec89` passes all four jobs in
[run 43259512674](https://github.com/eepyfishy/eef/actions/runs/43259512674),
including the required Python source/pipe checks, 1,771 Windows/headless Rust
tests, one ignored across 17 groups, and all 37 native suites plus existing gates.
Source and raw-log review accept that Python component only. It is not real-codec,
physical-camera, model-quality or owned-Rust-process acceptance.

The owned Rust transport adds strict bounds/field/order/image/end checks, local
start/ack backpressure, truncated/oversized output refusal, and redacted consumer
failure tests. Native fixtures hold real root and descendant process handles to
check success, failed-consumer retirement and expiry of the original deadline.
Exact source `7f487b6` passes all four jobs and source/raw-log review in
[run 43896291907](https://github.com/eepyfishy/eef/actions/runs/43896291907):
1,781 Windows/headless Rust tests, one ignored across 17 groups, all 37 native
suites and retained extra gates. This accepts the owned transport only.

The composed capture/model fixture failed the Windows workspace gate in
the `91951ba` and `3b6b4e2` runs; neither is accepted. Its trace writer used
platform-default newlines while the assertion expected LF. The current candidate
sets LF explicitly and reports a failed source immediately instead of waiting
for idle peer fixtures. Raw logs confirm the CRLF/LF mismatch in both runs;
the correction passes in `ac1717c`. No local Python, model or application execution
is used.

## Remaining integration

The next passive inspection candidate adds `eefn workloads vision-sources` and
`eef node vision-workload-sources --node NODE_ID`, backed by the local and
coordinator command APIs. A strict, bounded report composes camera configuration
with applied local vision model selections and owner policy. Saved/applied model
differences require restart and review. Runtime/session/freshness checks surround
the independently authenticated remote read; legacy peers cannot fall back to
camera-only inspection. No device or model is contacted, no private URL/path is
returned, and installed digests remain unverified until actual analysis. This
configuration read grants no execution authority. Six source regressions, two
CLI cases and extensions to control-capacity, legacy-session and native readiness
checks are pending hosted verification.

Metadata-only execution advertisements retain their exact old policy shape even
when local vision approval is enabled. That local opt-in cannot imply vision
support on an older wire version or make strict older peers reject unrelated
microphone/camera-motion registration. Vision execution still requires distinct
compatible negotiation; no network fallback is enabled.

The network feature remains incomplete. The private admitted driver is available
to internal preparation and synthetic fixtures; public offers and automatic source discovery
still refuse vision workloads. Next work needs compatible feature negotiation,
source selection, placement and command-based job review and result
retrieval. Approved bounded text can leave the node through the authenticated job
lifecycle; camera images must stay off coordinator traffic and history. Model
absence, revocation, expiry, stop, restart and lost replies must not cause fallback
or replay. Native end-to-end command and network tests are required before calling
the public workflow delivered.
The remaining physical, resource/strong-stop, installer and stable-release gates
remain open. No broader v0.5 information-flow, module or UI design is activated.
