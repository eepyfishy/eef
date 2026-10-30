# Owned Python observation streams

The node runtime now has a low-level streaming counterpart to its one-shot
Python bridge. An explicitly selected trusted plugin implements
`stream(action, params)` as an iterator. Rust delivers each bounded metadata
observation to its caller while the source is still running, rather than
buffering a complete recording before returning.

The trusted-library `observe_pinned_stream` counterpart accepts an immutable
`PinnedStreamSource`, constructed from UTF-8 bytes and their expected SHA-256.
It executes those bytes directly instead of opening a plugin path. Both Rust
and Python check source identity before the module can run. Encoded source bytes
count against the existing request limit, with a 48 KiB source ceiling. The
original path-based API remains available for trusted extensions. Neither path
is a network code-submission interface or a workload authorization mechanism.
Pinned module bytes do not pin imported packages or attest physical hardware.

Each invocation, including pinned execution, has a fresh canonical stream ID and an absolute caller deadline,
tightened by the existing local Python timeout. The protocol requires one ready
frame, ordered observations and one matching end frame followed by clean EOF and
a successful process exit. Readiness confirms the transport, not a device opening.
Each observation carries a monotonically increasing source-frame count, an
optional activity decision, an optional bounded level and a discontinuity flag.
Unknown activity stays unknown; a gap cannot assert a known activity decision.
Frames cannot carry raw audio, images, transcripts or arbitrary plugin fields.

Both hosts enforce frame, total output, observation and source-frame limits.
Rust waits for each observer callback before delivering another observation;
there is no detached consumer task or unbounded queue. Backpressure consumes the
same deadline. Plugin diagnostics remain private, and stdout/stderr retain the
existing independent byte limits. Python uses the existing isolated import and
disabled bytecode-cache flags.

On Windows the interpreter and descendants use the existing owned job, including
its configured commit-memory ceiling and kill-on-close lifetime. Successful
completion also observes owned-tree exit before the original deadline. Observer
failure, cancellation, malformed output and deadline expiry retire that owned
job. This does not establish that effects in shared services, network peers or
other external systems stopped, and it provides no rollback guarantee.

## Validation and remaining integration

The pinned-source development increment adds two Rust identity/input-bound
regressions. The native microphone-stream fixture now also executes the actual
energy iterator from reviewed bytes with a fake device, replaces/deletes a
nearby source path, and checks digest/schema/request-budget rejection before
source effects. Its ten cases retain the original three file-based checks.
Hosted compilation and native acceptance for this increment are pending; no
local heavy build or physical microphone test has been run.

Eight Rust tests cover framing, scope/order, limits, uncertainty, observer failure
and backpressure. `tools/test-python-stream.mjs` additionally uses an existing
trusted real interpreter with a synthetic producer. Its ten cases check live
delivery before EOF, private failures, output limits, the configured Windows
memory ceiling, iterator closure and cancellation/deadline cleanup. Four cases
hold creation-verified operating-system handles for both the producer and a
descendant and observe both exit. Requests, process outcomes, exact binary inputs
and held-observer results remain in each fresh validation directory.

This API is not a command or an advertised capability. Neither startup nor the
ordinary RPC dispatcher invokes it. It adds no grant renewal, permission,
listening setting, new network route or ambient capture default. No microphone or
camera adapter is shipped by this increment.

Continuous workloads still require integration with the existing durable job and
attempt lifecycle, separately scoped admission, owner-selected resources, local
listening controls, lightweight detectors, bounded retained payloads, event
deduplication/backpressure and coordinator reporting. Physical capture, resource
quotas, real-media stop and second-PC acceptance remain open. A series of finite
recording calls does not satisfy that requirement.
