# Node workload readiness and live registration

On Windows, a node can advertise its built-in continuous microphone source only
when its runtime is current, Python is configured, a supported microphone resource
is applied, microphone permission is saved and applied, local listening consent
is enabled, and workload policy permits it. This advertises configured readiness;
device access is checked when separately approved work starts. No model, capture,
lease, workload or restart is created by inspecting or enabling readiness.

Source reports use the existing schema-three executable-readiness form; unavailable
inspection reports retain schema two. The existing consent command uses schema two
only when its Windows/Python integration is present, with a matching explanatory
note and all no-execution flags retained. Old unavailable consent reports retain
schema one. Command confirmations refuse incompatible flag/schema combinations.

Relevant configuration saves invalidate a monotonic metadata version before I/O,
including a failed save, and notify again after successful publication. Policy and
listening changes, explicit latch re-enabling, runtime binding and runtime retirement
also notify. Unrelated permission switches and node-name edits do not trigger source
inspection. Original permission/listening/workload revocation latches remain the
execution fences; metadata versions cannot enable, revive or extend a workload.

One passive native inspection per service reads settings outside the Tokio worker
pool. Its actual capacity stays charged through completion after observer cancellation
or reconnect. It holds no live runtime-binding lock across storage and receives no
source, issuer, lease proof or Tokio runtime handle. Results must still match their
original configuration, gate and runtime version before use. A saturated or blocked
inspection can leave readiness unavailable while control traffic remains serviceable.

The connection coalesces metadata changes behind that one inspection. It retires
cached readiness immediately, publishes an unavailable registration when needed,
and publishes current readiness after inspection. A failed unchanged inspection
waits for a new change; it cannot create an automatic execution retry. Updates
preserve the authenticated session and partially received input frame. Model
registration updates apply the same current readiness check. Ordinary plugins
cannot advertise the built-in workload route by reusing its capability name.

Three shared regressions cover passive readiness/version retirement, cancellation
and actual storage capacity with prompt runtime retirement, and selective source
invalidation. A new opt-in Windows case uses the running NodeClient and NodeServer
over real independent credentials and pinned TLS, with the shipping source adapter
and existing synthetic device prelude. It exercises source completion and blocked
source stop, durable ACK/events/receipts, owned root/descendant exit, microphone
reuse, and policy disable/enable registration without reconnect or replay.

These new tests await changed-source hosted verification. The native peer uses
the original lease-window implementation directly; the coordinator's full command
driver and real node source still need combined acceptance. Synthetic device
preludes are unit-test-only, never shipping hooks or physical microphone evidence.
Physical two-PC/long-source, final main, the earlier capture timeout, installer and
remaining roadmap/release gates stay open. No installed app/node, release, version,
feed, model, permissions or OS policy is changed by this development increment.
