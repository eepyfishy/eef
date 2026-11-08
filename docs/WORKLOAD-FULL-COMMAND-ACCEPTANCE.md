# Combined workload command and source acceptance

Run `42426249194` reached approved camera execution after passing passive readiness
and the earlier restart checks, but the first job was interrupted before a consumed
ACK. Source inspection found the shared camera/microphone command event reservation
default exceeded a fresh node's receipt limit (256 KiB versus 64 KiB). The default
now fits that limit, with explicit larger requests still configurable and refused
when outside owner policy. Typed bounded admission diagnostics and regression
coverage accompany the correction. The failed run did not expose its refusal code;
fresh full hosted acceptance remains required.

An additional executable-level extension is in development in the existing
input-command native suite. It runs shipping services and public owner/origin
commands for camera completion, explicit stop and node restart/reconnect. Python
dependency installation is isolated fixture setup; binding, permissions, ambient
consent, workload limits, restart, submission, approval and execution use commands.
Independent JavaScript event-prefix reconstruction is compared with the real node
receipt. Exact execution reuse must not replay, new jobs need separate approval,
and earlier input-only jobs must remain unexecuted. This extension still needs
its own hosted acceptance and opens no physical camera.

The first executable run stopped before capture because readiness was false.
An additional startup correction recognizes explicit camera source resources
independently of local-webcam one-shot plugin discovery. It initializes only the
interpreter; permissions, consent and the exact approved source still control
execution. That correction and the full executable lifecycle remain under hosted
verification.

Run `42424184808` stopped earlier in the same native suite, at the answer-restart
uncertainty assertion. All workspace configurations passed, but the executable
camera fixture was not reached. Answer handling now shares the input workflow's
tested final-poll runtime fence so nested cancellation cannot conceal runtime
retirement. This correction and camera execution still require fresh hosted
acceptance; neither is inferred from the workspace test results alone.

The new opt-in Windows test joins the complete runtime path: explicit submission
and compilation through the already running origin node, owner HTTP conditional approval, the
coordinator's original engine-owned issuer/driver, credential-authenticated pinned
TLS, the actual NodeClient/NodeService/GrantLedger, and the shipping source adapter.
No dashboard, language model, physical microphone or physical camera participates.

The camera extension adds a separate Windows native test for the same three
lifecycle scenarios. It uses typed `CameraRequest` submission and real backend
camera selection, with only `cv2` simulated inside its disposable venv. The
shipping source/host bytes remain unchanged. Camera completion requires five
durable observations; the original microphone test still requires four. Both
retain exact payload-prefix and node-receipt comparisons, lease renewal, owned
exit, no replay after uncertainty and separately approved source reuse. The
extension passes at `1943155` in the complete
[four-job hosted run](https://github.com/eepyfishy/eef/actions/runs/42391526288),
including 1,600 Windows/headless Rust tests plus one ignored test, all 36 native
suites and the original additional gates. No physical camera is opened. The
separate five-event node-native completion rejection in `42388092617` remains
retained; later passes do not establish its cause or general physical reliability.

The origin-admission follow-up replaces direct test-helper review creation and
owner compilation with the actual concrete-input mailbox. Its built-in source
needs no one-shot audio plugin. The original `480e4ad` candidate used owner HTTP
compilation; acceptance of that checkpoint cannot establish the later input path.

The later typed microphone-authoring increment uses that same backend through
`MicrophoneRequest`, with coordinator-owned resource selection and immutable typed
intent. It replaces the fixture's caller-authored resource/adapter digests with
actual backend selection. Separate owner approval, origin execution and every
completion/stop/reconnect/no-replay assertion remain required. This later source
needs its own hosted acceptance and does not inherit a pass from older checkpoints.

The test creates a disposable stdlib venv from the explicitly pinned CI interpreter
using `--without-pip`. Each test installs only its synthetic device dependency
into that environment's site-packages. No dependency is downloaded, no system Python
is modified, and production adapter/host bytes and private test-hook boundaries
remain unchanged. The normal isolated Python invocation loads the test dependency;
no source factory or fixture selector is added to shipping code.

The scenarios exercise completion, owner stop and connection loss. The source waits
for observed coordinator consumption and an active renewal interval; successful
completion must report at least two actual leases. Each required event is released
only after it appears in the coordinator's durable job history. Completion must
match the real node receipt and exact definition-bound event count/bytes/prefix.
The existing node-native case separately verifies held process identities and exit.

The preceding readiness run `42179897667` failed its new Rust native lifecycle
assertion: the node journal checksum binds canonical records including lease and
timestamp metadata, while the coordinator receives event payloads only. The three
other hosted jobs and all 36 subsequent native scripts passed; the full run failed.
This change keeps the original record checksum and derives a second payload-only
prefix from validated retained node rows. Schema-two completion reports carry both;
the coordinator compares the payload prefix with its exact durable observations.
Neither counts alone nor the old record checksum can substitute for this evidence.

The existing journal schema and historical schema-one reports remain readable
without rewriting or inventing payload checksums. New schema-two reports require
the payload checksum; mismatched schemas and damaged payload prefixes refuse.
Registration explicitly advertises payload-completion support. A new coordinator
refuses older experimental peers before issuing a workload, and old strict readers
refuse new metadata rather than starting work they cannot validate. Historical
coordinator records containing observations but lacking matching payload completion
evidence are conservatively refused, never repaired or replayed.

Stop and disconnect must preserve unknown execution and the original consumed ACK.
Stop retains an exact intent-only node receipt, never a fabricated physical-stop
claim. Reconnect must use a new authenticated session and retain the old history.
Reusing the original command returns its receipt without executing again. A fresh,
separately reviewed/approved workload then proves source reuse while the old
uncertain job remains unchanged. Fresh trace directories prevent old markers from
releasing a later source prematurely.

A parent `eef/native-test-fixtures` feature enables this unit test and forwards the
existing node fixture feature. Only the Windows/headless workspace test step adds
that parent feature, alongside the existing child flag and pinned Python. Normal
builds retain their existing defaults. The strict CI guard verifies both opt-in
features and the adapter-test placement; all original native suites, failure gates,
source-host restrictions, deadlines and dependency locks remain required.

Candidate `480e4ad` passed all four jobs in hosted run `42184728406`, including
Rust1501+1ignored across 17 Windows groups and every required native suite/gate.
That checkpoint verifies the separate payload checksum and the combined workflow
using owner compilation. The later origin-admission/mailbox follow-up requires
its own changed-source hosted verification. This is a synthetic device
workflow with a normal temporary Python environment, not evidence for a physical
microphone, packaged installer, long source, partition behavior or two physical PCs.
The earlier main capture deadline failure and remaining stable v0.4.0 implementation
and release requirements remain open.
