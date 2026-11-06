# Combined workload command and source acceptance

The new opt-in Windows test joins the complete runtime path: owner HTTP compile
and conditional approval, input through the already running origin node, the
coordinator's original engine-owned issuer/driver, credential-authenticated pinned
TLS, the actual NodeClient/NodeService/GrantLedger, and the shipping source adapter.
No dashboard, language model or physical microphone participates.

The test creates a disposable stdlib venv from the explicitly pinned CI interpreter
using `--without-pip`. It installs only the existing synthetic microphone dependency
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
separately reviewed/approved workload then proves microphone reuse while the old
uncertain job remains unchanged. Fresh trace directories prevent old markers from
releasing a later source prematurely.

A parent `eef/native-test-fixtures` feature enables this unit test and forwards the
existing node fixture feature. Only the Windows/headless workspace test step adds
that parent feature, alongside the existing child flag and pinned Python. Normal
builds retain their existing defaults. The strict CI guard verifies both opt-in
features and the adapter-test placement; all original native suites, failure gates,
source-host restrictions, deadlines and dependency locks remain required.

Full changed-source hosted verification is pending. This is a synthetic device
workflow with a normal temporary Python environment, not evidence for a physical
microphone, packaged installer, long source, partition behavior or two physical PCs.
The earlier main capture deadline failure and remaining stable v0.4.0 implementation
and release requirements remain open.
