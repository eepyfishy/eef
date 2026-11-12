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

## Verification and remaining integration

Eleven source tests use deterministic camera/image/clock fakes. Ten private pipe
cases test start/ack gating, slow consumption, image transfer, legacy EOF before
start, wrong acknowledgements, oversized/duplicate controls, source hash/error,
early end and unsolicited images. The existing required native source suite
invokes them after its original microphone/motion checks. Local JavaScript syntax
and diff checks pass; hosted Python/process verification is pending. This is not
real-codec, physical-camera, model-quality or owned-Rust-process acceptance.

The feature remains incomplete. No public source registration, startup capture,
model call, additional permission or network image path is enabled by these files.
Next work must connect this pipe to the existing owned capture/grant/lease path,
bind an explicit selected model and image/call/token/result budgets, enforce local
endpoint handling, persist analysis decisions before model effects, and deliver
bounded approved results through the existing durable lifecycle. Model absence,
revocation, expiry, stop, restart and lost replies must not cause fallback or
replay. Public node commands, separate owner approval, actual native node/ledger
integration and negative cases are required before calling the workflow delivered.
The remaining physical, resource/strong-stop, installer and stable-release gates
remain open. No broader v0.5 information-flow, module or UI design is activated.
