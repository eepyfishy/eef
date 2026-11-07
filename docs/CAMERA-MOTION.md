# Camera motion source in development

`python/streams/camera_motion.py` implements a lazy, single-open motion iterator
for later admitted camera workloads. It samples a small grayscale image, compares
it with the preceding sample and emits only frame count, optional activity,
optional difference level and discontinuity. This detects image change, including
lighting changes; it does not recognize a person, identify presence or locate a user.
No image, video, endpoint, transcript or model output leaves the iterator.

The first image is unknown. Configurable activation/deactivation thresholds retain
hysteresis between decisions. A changed input resolution invalidates the preceding
comparison and reports an unknown discontinuity. A failed read fails the source;
it never invents a quiet frame, reopens the camera or switches backends. Frame
counts describe successfully returned and validated images, including images that
establish a new baseline. Raw images are discarded before yielding. Only one small
previous grayscale sample remains local; this is not an offline video cache.

| Setting | Default | Bound |
| --- | --- | --- |
| Device | Webcam index `0` | Index `0..1023`, or an explicitly bound `rtsp`/`rtsps` endpoint |
| Requested / maximum accepted size | `640 x 480` | Up to `1920 x 1080`; actual frames must fit the configured maximum |
| Comparison size | `64 x 48` | Width `8..160`, height `8..120` |
| Maximum valid frames | `100` | `1..100000` |
| Minimum read interval | `200 ms` | `100..10000 ms`; no catch-up reads after backpressure |
| Activation / deactivation | `15 / 5` | Mean absolute grayscale difference `0..255`; deactivation cannot exceed activation |
| Local duration budget | `60000 ms` | `100..3600000 ms` |
| RTSP open/read timeout hints | `2000 ms` | `100..10000 ms`, at most the local duration budget |

All parameters reject wrong types, unknown fields and invalid bounds before
device creation. File paths and other URI protocols are refused. RTSP uses the
explicit FFmpeg backend with open/read timeout parameters and audio disabled;
unsupported environments fail without a fallback. See OpenCV's
[capture parameters](https://docs.opencv.org/4.12.0/d4/d15/group__videoio__flags__base.html).
The private endpoint must eventually come from an owner-approved resource binding,
not an arbitrary request field passed through the coordinator.

OpenCV transforms use one thread in the source's own process. See its
[thread setting](https://docs.opencv.org/4.x/db/de0/group__core__utils.html).
Decoder/driver buffers, decoder threads, actual frame quality, hardware deadlines
and total working set require separate resource and physical acceptance. Device
properties are requests, so every returned frame is independently checked for
dimensions, three channels, unsigned bytes and exact logical byte count before
sampling. The comparison retains at most two tiny grayscale byte samples while
computing a difference; this is not a bound on all decoder/process memory.

The local monotonic budget includes open and reads and never resets between
images. It supplies no work authority or extra time beyond the caller's original
deadline. A blocked native open/read/release still requires the outer owned process
deadline, cancellation and observed termination. Timeout properties alone do not
prove physical driver release or external effects stopped.

There is no one-shot handler, startup registration, camera workload command or
ambient capture default. Existing one-shot camera capture is unchanged. Integration
must pin these source bytes, retain an exact camera resource/adapter definition,
check camera permission and local listening policy, and use the existing durable
job/grant/lease/event lifecycle. Camera events must have camera semantics, rather
than being labeled as microphone activity. Source reservation/revocation, readiness,
JSON-free authoring, local event-triggered VLM, relay paths, aggregate quotas and
physical two-PC/webcam/RTSP acceptance remain required for the complete workflow.

The required native source suite now invokes 19 fake-device tests and three cases
through the existing Rust/Python owned streaming transport: ordinary changes,
resolution discontinuity and read loss. Tests execute pinned reviewed source bytes
with a replaced-path canary and check bounded metadata, one open/release, no retry,
failure retention and no image payload. They use fake OpenCV/device APIs without
installing OpenCV or touching a camera/network. The producer and archive-order
source `1c7016e` passed all four jobs and complete native gates in
[run 42334853657](https://github.com/eepyfishy/eef/actions/runs/42334853657).
These tests do not establish physical capture, admission or VLM/relay acceptance.

## Reviewed definition and event integration in development

The existing version-two workload definition now has a closed `camera_motion`
source with the producer's exact motion parameters, node-qualified resource ID,
private-resource digest and adapter digest. Private webcam/RTSP selection resolves
only on the owning node. Local source duration cannot exceed the reviewed session;
frame count, parameter ranges and existing memory/event/lease bounds apply.
Microphone serialization and its sample-frame meaning remain unchanged. Older
readers reject the new source/event kind; camera history requires a compatible
reader and must never be relabeled or stripped to make a downgrade accept it.

The existing event filter, payload-prefix checks, durable event journal and
completion metadata validation now use the definition's observation shape.
Camera observations count one valid image each, use motion levels `0..255`, and
produce `camera_motion` events. Microphone activity keeps its original block stride,
RMS range and kind. Discontinuities remain unknown, invalid observations retire
the filter, and retained camera rows confer no source/restart authority. Generic
review capability checks now derive `camera.capture` from camera intent.

This integration does not yet expose camera execution: microphone preparation,
microphone source inspection and typed microphone authoring explicitly refuse
camera definitions. The camera-specific permission/listening controls and shared
one-shot source ownership are being implemented below. Camera readiness,
prepared-source ownership, compatible peer negotiation and the owned source runner
still need integration before the existing command/compiler/lease path may launch one.
Ten shared regressions cover binding/limits, event/prefix behavior, durable history
reopen/no replay, old microphone shape and those refusal boundaries. Hosted
verification is pending; no installer, installed node or runtime policy is changed.

The exact definition/filter/history source `f2df98e` subsequently passed all four
jobs in [run 42336088398](https://github.com/eepyfishy/eef/actions/runs/42336088398),
including Rust1534+1ignored across 17 groups, all 36 native suites, camera19+3
and the complete prefix/firewall/listening/first-run gates. This verifies that
synthetic source revision, without accepting later integration or physical devices.

## Independent ambient consent lifetime in development

The local listening preference governs both ambient camera and microphone work;
each source additionally needs its own saved and applied feature permission.
Camera supervision now has a separate revocation epoch, so disabling microphone
permission leaves authorized camera work alone. Camera permission denial retires
only camera consent; local listening-off retires both. Denial occurs before config
storage, including failed writes. Restoring permission through a generic save
cannot revive an operation or clear the runtime denial. A fresh explicit local
listening-enable decision can clear that latch, while old prepared epochs remain
invalid. Replacing or dropping the owning runtime also retires camera operations,
including replacement with the same runtime ID. Stale guards cannot retire the
replacement's consent or remove its binding.

Six synthetic library-level cases cover permission separation, saved/applied/off
checks, deadline/identity refusal before polling, owned-future drop on revocation,
failed persistence, off/on and runtime replacement. They start no hardware capture
or subprocess and grant no workload, resource or extra deadline. Hosted verification
is required. The public listening report retains its existing microphone-specific
compatibility fields; camera readiness and command exposure must be integrated
separately before camera workload execution is advertised or launched. Existing
one-shot capture and its permission behavior remain separate.

## Shared Windows camera source ownership in development

Windows one-shot `camera.capture` now reserves the node service's camera pool
before invoking its Python plugin. Engines belonging to the same service share
one conservative camera reservation, independent of the microphone reservation.
This excludes competing captures by this node across engine instances and resource
aliases; it does not lock other applications or claim per-driver exclusivity.
Preparing a camera runtime must match both the resource owner and this exact pool;
attaching another service replaces both source-pool bindings.

The existing reserved bridge holds the camera permit through observed owned-job
exit, including descendants. Requesting termination does not release it; absent
positive exit evidence, the existing quarantine remains. A library lookup or
unarmed reservation starts no process and grants no feature/workload permission.
Non-Windows one-shot behavior is unchanged; owned workload capture still requires
platform support. There is no camera workload launch or new advertisement yet.

Five new hosted cases cover separation, owner/service identity, replacement,
busy dispatch before plugin/interpreter use, and synthetic one-shot execution.
The synthetic Python fixture emits a one-pixel PNG only after a test release
barrier, allowing the actual engine dispatch to prove exclusion during execution
and shared-pool reuse after observed exit in two sequential calls. Each keeps its
six-second parent deadline. No physical device, OpenCV or model is used; physical
driver release, camera workload stop and RTSP acceptance remain unverified.

## Source-specific coordinator preflight in development

The coordinator now compares the reviewed source kind with the authenticated
peer's validated workload advertisement in both session preflight and direct route
creation. Generic workload or one-shot camera capability is insufficient. The
existing version-two advertisement remains microphone-only, with unchanged fields
and serialization; forging `camera_motion` under that version fails validation.
Camera requests are refused before allocating route capacity or sending an offer,
including callers that bypass the normal source-inspection compiler path.

Three synthetic regressions cover kind/serialization agreement, preflight with
otherwise valid pinned session/runtime/policy/stop metadata, and direct prepare/open
refusal without capacity consumption or outbound traffic. Microphone routing must
still work afterward. Hosted verification is required. This check introduces no
camera advertisement, lease, authority, fallback or automatic retry; actual camera
negotiation must be added together with its supported runner and readiness path.

## Passive camera inspection commands in development

`eefn workloads camera-sources` inspects the running local node;
`eef node camera-workload-sources --node NODE_ID` reads it through the coordinator.
Both use backend operations with separate closed camera reports, including saved
and applied camera permission, camera consent epoch, public resources and private
binding/adapter digests. A configured Python interpreter is not an OpenCV, driver
or endpoint readiness claim. No device is imported, opened, contacted or scanned;
there are no preference writes, grants, model calls, restarts or automatic retries.
This report protocol explicitly refuses any claim of camera workload execution.

Private resource parameters/properties and RTSP credentials/endpoints are omitted.
Pending saved edits do not substitute an applied source: their differing binding
status is visible, and exact reviewed-template confirmation refuses the mismatch.
Runtime replacement, including reuse of its ID during a blocked config read,
invalidates the old result. Camera permission changes and camera epochs invalidate
source-readiness snapshots; an initial denial also wakes the existing readiness
observer, so unrelated saves cannot silently strand microphone readiness.

Remote reads require independently authenticated pinned sessions, fresh
registration and stable session/runtime identities before and after the request.
Legacy shared-secret channels are refused, and both source reads share the existing
bounded control lane. Local HTTP/CLI adapters validate the exact requested camera
report; older APIs and lost/substituted replies cause errors without microphone
fallback or retry. The coordinator command reads the same node-owned snapshot.

Ten new Rust regressions and additional mandatory native executable checks require
hosted verification. Coverage includes privacy, malformed/scoped reports, exact
camera template matching, pending edits/revocation, blocked inspection replacement,
actual pinned TLS reads, readiness notifications, control capacity and both CLI
parsers. Executable checks run both commands with no dashboard pages, then test
human output, malformed requests and old/forged/lost replies. They use configured
synthetic resources without Python, OpenCV, cameras or models. Camera resource
configuration/authoring, admission/runner, compatible execution negotiation,
event-triggered VLM, relays and physical acceptance remain required.

## Private prepared camera runner in development

Camera preparation now checks the exact applied node/resource owner, saved/applied
camera permission, local listening consent and camera epoch. It pins the compiled
motion source and private bound parameters and reserves the same camera pool as
Windows one-shot capture. Microphone capacity remains independent. Preparation
opens no device, grants no workload and cannot replace a reviewed source. Local
memory intersects the reviewed budget; source frames, observation count, event
bounds and all original deadlines remain.

The private admitted runner uses the existing workload consumption, original lease
and session supervisors, reporting authority, durable event writer and filtered
completion seal. It confirms the current camera snapshot before polling its source
and uses the camera consent gate. No typed receipt or old epoch can invent source
or exit evidence. Every callback follows its persisted event and completes before
the next observation is read. Completion requires the original exact source bytes,
matching camera metadata and observed root/descendant exit; it does not claim
physical effects stopped. Revocation, scoped stop and whole-future cancellation
retain uncertainty and hold the reservation until observed exit or quarantine.

Six Windows unit regressions and three explicit native cases, plus one unsupported-
platform refusal, require hosted verification. The native fixture injects only
the fake OpenCV/device API into the actual pinned producer via the existing
test-only prelude. It opens one simulated RTSP camera and real owned processes,
paces five required metadata transitions from durable callback acknowledgements,
and exercises normal completion, blocked-read revocation/scoped stop and complete
future drop. Reopening the receipt journal must preserve history without granting
replay. No OpenCV installation, real endpoint, webcam, model or physical driver is
used. All existing microphone regression source remains, including bounded futures
and original native timing limits.

This is private library integration. The network driver still prepares microphone
workloads only; current execution advertisements and coordinator source checks
continue to refuse camera workloads. The passive camera report still says execution
unavailable. Supported camera negotiation/readiness and explicit command authoring
must be integrated before the public lifecycle may use this runner. VLM triggers,
relays, aggregate quotas and physical two-PC/webcam/RTSP acceptance remain open.
