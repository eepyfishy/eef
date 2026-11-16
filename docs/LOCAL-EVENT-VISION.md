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

## Verification and remaining integration

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

The new Rust candidate adds strict bounds/field/order/image/end checks, local
start/ack backpressure, truncated/oversized output refusal, and redacted consumer
failure tests. Native fixtures hold real root and descendant process handles to
check success, failed-consumer retirement and expiry of the original deadline.
Hosted compilation and execution of this Rust candidate are pending; no local
Python, model or application execution is used for this checkpoint.

The feature remains incomplete. No public source registration, startup capture,
model call, additional permission or network image path is enabled by these files.
The private adapter is not exposed by a command, grant driver or startup path.
Next work must connect this owned pipe to the existing workload/grant/lease path,
bind an explicit selected model and image/call/token/result budgets, enforce local
endpoint handling, persist analysis decisions before model effects, and deliver
bounded approved results through the existing durable lifecycle. Model absence,
revocation, expiry, stop, restart and lost replies must not cause fallback or
replay. Public node commands, separate owner approval, actual native node/ledger
integration and negative cases are required before calling the workflow delivered.
The remaining physical, resource/strong-stop, installer and stable-release gates
remain open. No broader v0.5 information-flow, module or UI design is activated.
