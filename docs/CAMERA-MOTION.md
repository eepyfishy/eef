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
installing OpenCV or touching a camera/network. Hosted verification is pending;
these tests do not establish physical capture, admission or VLM/relay acceptance.
