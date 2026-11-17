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

## Private admitted runtime checkpoint

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
Media, encoding and model responses are synthetic; verification is scoped below.

The corrected private runtime and passive commands at `943b67b` pass all four jobs
and scoped source/raw-log review in
[run 43991650485](https://github.com/eepyfishy/eef/actions/runs/43991650485):
1,821 Windows/headless Rust tests, one ignored across 17 groups, all 37 native
suites and retained extra gates. This covers explicit owner policy, admitted
synthetic capture/model composition, joined completion and passive commands.
The earlier quiet-startup exit was not reproduced in the next two command runs;
its cause is still unproven. No physical device/model or stable release is accepted.

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
the new kind. The private admitted driver and joined completion have scoped
synthetic verification, with explicit owner opt-in. Compatible network
negotiation, source selection, placement and public job/result commands remain required before
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

The verified passive inspection path adds `eefn workloads vision-sources` and
`eef node vision-workload-sources --node NODE_ID`, backed by the local and
coordinator command APIs. A strict, bounded report composes camera configuration
with applied local vision model selections and owner policy. Saved/applied model
differences require restart and review. Runtime/session/freshness checks surround
the independently authenticated remote read; legacy peers cannot fall back to
camera-only inspection. No device or model is contacted, no private URL/path is
returned, and installed digests remain unverified until actual analysis. This
configuration read grants no execution authority. Six source regressions, two
CLI cases and extensions to control-capacity, legacy-session and native readiness
checks pass at the `943b67b` checkpoint above.

## Bounded result delivery in development

The next backend candidate adds separately negotiated historical analysis pages
over the existing pinned credential channel. Each read binds the original issuer,
node and exact historical grant scope; the old session is evidence, not current
execution authority. Pages carry at most 16 records below the existing 128 KiB
control bound, with exact cursor and checksum-prefix correlation. Records contain
approved bounded text, image digests and metadata, never camera bytes or private
bindings. Existing receipt inspection keeps its old wire shape. Older peers have
no result-read fallback.

Node reads use the existing bounded receipt-control allowance, retained by the
actual native reader after waiter loss. They do not write, repair, renew, replay
or reconcile a job. Interrupted/pending history remains explicitly uncertain.
A completed-history value verifies every record, full ordering, aggregate bytes,
the joined completion and consistent headers across pages; deserializing that
value cannot manufacture the journal's private source witness or analysis permit.
Six common regressions and an extension to the actual admitted native fixture are
pending hosted verification. Public job commands, coordinator persistence and
vision execution negotiation remain unfinished.

Initial result-read candidate `1782e00` fails three common hosted tests: two
control requests used hyphenated IDs where the existing frame expects compact
IDs, and one authenticated fixture timed out without observing its client's
exit or actual journal availability. The correction keeps the existing compact
ID contract, waits for the real uncertain checkpoint in its fixture and reports
early connection/journal failures directly. It also bounds completed-history
assembly under a caller deadline and redacts retained text in diagnostic
formatting. This result-read candidate remains unaccepted pending new hosted gates.

The next hosted run isolates the remaining failure: the fixture's advertised
runtime and live runtime status differed, so the existing registration fence
correctly rejected it. The fixture now pins the same generated runtime in both
places; production registration checks are unchanged.

The coordinator persistence candidate records the selected model/backend in
planning and assignment, preserves the reviewed definition version and reserves
combined text/metadata storage before issuance. Vision completion requires both
the original durable event prefix and a full matching completed analysis history.
Missing or changed results cannot complete work, and a saved result cannot create
a new issuer. Metadata-only completion serialization remains unchanged.

The existing `requests result JOB_ID --grant-id GRANT_UUID` command reads completed
workload data from the coordinator journal, including approved analysis text when
present. It checks the immutable origin and exact grant without contacting the
executing node. Event metadata is summarized to keep command replies bounded;
the full event prefix remains validated in the journal. Results remain readable
when the executing node is offline. Four coordinator regressions exercise
issuance/opt-in, durable reopen/origin checks, incomplete/mismatched completion
and storage refusal before issuance. Exact commit `9603e47` passes all four hosted
jobs in [run 44003140181](https://github.com/eepyfishy/eef/actions/runs/44003140181),
including 1,831 Windows/headless Rust tests, one ignored across 17 groups,
all 37 native suites and the retained extra command/camera gates. This accepts
synthetic result transport/persistence and passive review, not physical/model
quality or full public vision execution.

Generic source review now selects the existing authenticated vision inspection
endpoint for a vision template and checks the exact camera/model binding. The
typed snapshot at that checkpoint remains review-only: configuration readiness
alone never satisfies execution readiness, and a camera-motion advertisement
cannot authorize vision.
The existing authenticated snapshot regression also exercises this selection
and rejection of an unselected model. Payload-bound metadata results must retain
their summary; only legacy completion records can omit that newer evidence.

Metadata-only execution advertisements retain their exact old policy shape even
when local vision approval is enabled. That local opt-in cannot imply vision
support on an older wire version or make strict older peers reject unrelated
microphone/camera-motion registration. Vision execution still requires distinct
compatible negotiation; no network fallback is enabled.

The next transport candidate adds a distinct `workload_vision_execution`
advertisement, binding runtime, owner policy and definition/offer/completion/read
versions. It is separate from unchanged metadata advertisements. Readiness is
published only for current applied camera/model configuration and explicit owner
opt-in, and updates in place after revocation. A vision-only inspection failure
does not remove unrelated metadata sources. Configuration readiness does not
prove a provider/model/device is physically available.

Vision routing requires the separate protocol plus current grant journal,
retained-analysis reads and stop support. Vision definitions use offer version
three; metadata definitions retain version two. Neither path can upgrade or
downgrade to the other. Source review joins its passive configuration view with
the separately negotiated support; source preparation and live inference retain
their existing node-local permissions, epochs, digest checks and lease bounds.
Images remain node-local. Five common regressions cover negotiation, version
substitution, missing dependencies, runtime fencing and route preparation. A
Windows native synthetic camera/model test exercises actual credential/TLS
execution, durable pre-inference intent, bounded result reads, owner withdrawal
and continued historical reads. This candidate awaits hosted verification.
The initial run passed Linux, but Windows found an issuer-deadline fixture type
error and a separate peer-driver observation timeout. The fixture now names the
correct standard-library clock, and the peer test adds failure-only state
diagnostics without changing its deadlines. Neither failure is a passing gate.

The next command candidate adds `eefn requests submit-vision`: an explicit camera,
local backend/model, installed Ollama digest, prompt and bounded image/analysis
budgets create an immutable review through the existing origin mailbox. It uses
the vision snapshot rather than falling back to camera-only inspection. Model
selection must already be applied; authoring never queries a provider, installs,
loads, captures, infers, changes permissions or authorizes execution. The default
metadata budget is 32 KiB plus at most one 1 KiB text result, within a fresh node's
64 KiB result policy. Larger reviewed budgets remain limited by owner policy.

The camera intent's new version two contains a strict optional vision block;
older metadata intents omit it unchanged. Version-two vision plan input explicitly
reviews `vlm.analyze` alongside the source/session capabilities. Earlier private
version-one vision reviews retain their original capability vocabulary and remain
inspectable, but cannot be newly compiled, approved or executed. They need a new
explicit review, never an automatic rewrite or renewed grant. Exact retained
receipts stay historical. Four common authoring regressions, an authenticated
coordinator mailbox/journal case, a CLI case, the one-MiB parser gate and an
actual CLI refusal without a selected model accompany this candidate. Hosted
verification remains pending.
An initial end-to-end authoring failure also exposed the old node-side plan-view
rule that refused workload model bindings. It now checks the exact nested vision
selection, including the generic binding validation pass. Missing/substituted
model IDs or backends still deny confirmation; metadata and one-shot rules keep
their prior scope. The corrected path requires new hosted verification.
The corrected authoring code passes Linux at `5fb4712`; Windows exposed a stack
overflow in shared admission setup. A heap-owned common command phase and boxed
fixture initialization keep nested callers bounded. A fixed 64 KiB future-size
check accompanies the actual admission tests; no larger thread stack or changed
execution/cancellation policy is used. Windows verification remains pending.

An additional Windows native command-service case now follows the complete
review, owner approval, original coordinator issuer, authenticated node execution
and result delivery path. It uses the shipping source with disposable synthetic
camera/encoder and model endpoints. The model asserts a durable intent exists
before its request; a held response exercises lease renewal. The coordinator
must persist matching events and analysis, deliver text to the originating node,
refuse a mismatched result origin, and read its checkpoint after the executing
node disconnects. Reusing execution must leave the job and model-call count
unchanged, and camera bytes/private endpoints must be absent from stored/output
data. Exact source `e2f269b` now passes all four hosted configurations in
[run 44090075565](https://github.com/eepyfishy/eef/actions/runs/44090075565),
including all 37 native suites and the retained extra command/camera gates.
Scoped review accepts this synthetic command-service completion path, typed
authoring and separate negotiation. It does not accept physical/model quality,
positive binary vision execution or stable readiness. Earlier pending notes
above describe their original checkpoints; the peer timeout's cause is unresolved.

The next result candidate distinguishes an interrupted coordinator job from its
node's retained evidence. `requests result` can read a bounded frozen analysis
prefix, including pending/unknown rows, through the existing authenticated
original-issuer route. It returns retained coordinator metadata if the executor
is unavailable. Even a completed node report leaves `job_completed` and
`outcome_confirmed` false. There is no reconciliation, source/model execution,
lease renewal or replay. Scope is checked before and after the bounded node wait;
missing/changed pages are not stitched into a supposed complete history.
The interrupted-result core at `b16e053` passes all four hosted jobs in
[run 44096657632](https://github.com/eepyfishy/eef/actions/runs/44096657632),
including 1,848 Windows/headless Rust tests, all 37 native suites and the retained
extra gates. Scoped review accepts the bounded historical reader, independent
evidence semantics and offline metadata path. Joined executable vision/stop
delivery and the assembly-deadline follow-up below remain separately pending.

An additional shipping-executable fixture now configures the owner-selected VLM
and vision policy through commands, authors and approves an explicit review,
and exercises completed text delivery and a stop during a pending model request.
It requires original receipt reuse to leave both the job and model-call count
unchanged. Only the disposable Python dependency and loopback model endpoint are
test setup; no hardware or actual model participates. Source `2f9ad02` passes all
four hosted jobs in [run 44098166237](https://github.com/eepyfishy/eef/actions/runs/44098166237),
including all 37 native suites and the retained extra gates. Scoped review
accepts positive executable completion and pending-stop result delivery.
Bounded result assembly checks the caller's same read deadline after full
validation, and expired-phase reads have regressions.

The public workflow remains incomplete. Next work needs model placement/capacity
integration and the remaining command/network failure cases, including lost model
replies. Completed, stopped and restarted command workflows now have synthetic
acceptance as recorded below; real models/devices remain separate.
Approved bounded text can leave the node through the authenticated job
lifecycle; camera images must stay off coordinator traffic and history. Model
absence, revocation, expiry, stop, restart and lost replies must not cause fallback
or replay. Native end-to-end command and network tests are required before calling
the public workflow delivered.
The remaining physical, resource/strong-stop, installer and stable-release gates
remain open. No broader v0.5 information-flow, module or UI design is activated.

A further executable fixture extends the same workflow through node restart and
reconnect. A third camera read is deliberately blocked only after the model text
has been durably retained. The restart must interrupt the job, preserve that text
in an explicitly unconfirmed result, and permit exact execution-receipt reads
without new capture or inference. Source `a875b15` passes all four hosted jobs in
[run 44134810348](https://github.com/eepyfishy/eef/actions/runs/44134810348),
including all 37 native suites and the retained extra gates. Scoped review
accepts this synthetic same-node input/executor restart workflow. It does not
establish distinct-node or physical acceptance.

A distinct-node extension runs those scenarios with a separately authenticated
input node and camera/model executor. It requires immutable input-origin context,
results delivered only to that origin, executor restart/reconnect with retained
text, and no fallback or replay. The input node has no local model, camera
permission or listening consent; only the source node is configured to capture.
Both processes use disposable simulated devices
on the hosted machine; this does not establish physical two-PC acceptance.
Its initial `9b2673b` native run was refused before review because the fixture
had not obtained the existing directional disclosure grant. The correction
retains that denial check, then applies owner approval and the required
coordinator restart through commands. This changes fixture setup, not source
disclosure or capability permissions. Corrected source `fe3a642` passes all four
hosted jobs in [run 44141704685](https://github.com/eepyfishy/eef/actions/runs/44141704685),
including 1,849 Windows/headless Rust tests, all 37 native suites and the retained
extra gates. Scoped review accepts this distributed command workflow and the
validated readable node/owner result output, with unchanged explicit `--json`.
The intermediate `c6b3566` run reached passing same-node text commands but retained
the missing disclosure and failed overall; this later success does not relabel it.

A further command fixture closes the model connection after receiving one
analysis request. It requires an interrupted job with a pending retained intent,
no analysis text or fabricated completion, working node controls and unchanged
camera/model counts after result and exact-receipt reads. It runs for both the
same-node and distinct-node layouts. This lost-reply extension awaits hosted
verification and does not claim termination of an independent model service.
