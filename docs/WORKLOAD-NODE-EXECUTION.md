# Continuous workload delivery on the node

The node now has a schema-two offer handler and session-local lease correlator in
its existing encrypted connection loop. Offers must match the actual pinned
issuer, node, authenticated session and registration runtime; legacy transport,
changed scope and unsupported envelopes refuse before a task starts. Reconnect
creates a separate correlator and cannot reconstruct proofs from historical rows.
Production registration still advertises no public continuous execution. The
coordinator owner start/stop path and full credential/TLS execution acceptance must
be connected and verified before that capability becomes available.

The node owner can configure admission through the running backend:

```powershell
.\eefn.exe workload-policy show
.\eefn.exe workload-policy enable --max-session-ms 60000 --max-lease-ms 10000
.\eefn.exe workload-policy disable
```

The preference defaults off. Omitted limits preserve their inspected values.
Commands use exact node/runtime, saved revision and runtime epoch checks; limits
retain the existing absolute protocol caps. The HTTP adapter is
`/api/commands/workload-policy`. Authenticated coordinator adapters may use
`node.configure/workload_policy` only with the node owner's remote-management
consent; `workload_policy_status` accepts inspection only. These controls use
reserved control capacity, so revocation does not wait for a free source slot.
The coordinator CLI adapter for this new policy remains to be added.

Saving changed limits or disabling revokes original policy permits before storage,
including a failed save. A generic save/restore cannot enable the preference,
widen active limits, reset the revision or clear a revocation latch. A fresh typed
owner command can clear the latch after inspecting the current state. Off/on
cannot revive an old permit. These commands grant no microphone permission,
listening consent, workload approval, source startup or restart.

Delivery follows this order:

1. Reserve the existing bounded work/reply admission. Prepare the exact pinned
   source under current policy, permission, listening, resource and runtime checks.
   Preparation opens no device. Actual native preparation retains execution
   capacity across cancellation/runtime shutdown; its waiter is bounded by the
   reviewed maximum lease duration. There is no detached running source or
   unbounded preparation queue.
2. Obtain the original initial lease through the session's challenge correlator.
   Await the existing bounded durable consumption and fresh saved-ACK inspection.
   The original proof and fixed deadlines remain on the caller throughout.
3. Successfully flush the consumed ACK within that original initial deadline.
   Only then poll the prepared source through its existing listening/stop/lease
   supervisors. A lost or partial ACK never authorizes a source retry.
4. Drive source execution and renewal as two futures in the same connection-owned
   task. Renewal scheduling uses the actual last proof's monotonic remaining
   budget. Every proof still passes the original runner's scope/sequence/window
   checks; renewal cannot extend the original session horizon. Source completion,
   failure, policy revocation, stop or disconnect drops the exact renewal future
   before terminal output.
5. Send one bounded event callback only after its durable journal append. Successful
   completion uses the existing owned-exit/filter-seal receipt. Failure sends a
   bounded rejection and retains uncertainty; it does not claim physical effects
   stopped. Final reporting may still be pending when rejection is delivered.

Coordinator transport now delivers rejection before queued observations and
refuses later lease replies on that link. Successful completion still drains the
already received bounded event prefix. Dropping the link releases queued budgets;
unrelated ordinary connection control remains available.

New regressions cover legacy/changed-scope refusal, work/control capacity,
default-off policy, actual preparation ownership across runtime destruction,
policy CAS and generic-save restrictions, live revocation and failed saves, and
immediate rejection delivery. Two opt-in Windows cases use the shipping source
preparation/adapter/process host with the existing explicit synthetic microphone
prelude. They assert durable consumption and no source at ACK flush, a real renewal
while capture waits, durable framed events/completion, and stop of real held
root/descendant processes before microphone reuse. Their encrypted duplex peer is
a dispatch unit fixture; it is not a credential/TLS connection or physical-mic
acceptance claim. Existing separate pinned-TLS transport tests remain required.

Run `42133115874` also exposed a Windows history-command fixture requiring a live
issuer from a two-second setup observation. The history fixture now stages exact
synthetic retained execution metadata through the real job journal, with no live
issuer window, proof, source or dispatch. Owner HTTP/origin command equality,
origin isolation, unchanged coordinator state and no execution/renewal/stop
assertions remain. Live issuance and its deadline/lifetime tests are unchanged;
no production timeout or conservative proof margin is widened.

Full changed-source hosted validation is pending. The existing main capture
deadline failure remains unresolved. Owner execution commands, public capability
advertising, genuine credential/TLS end-to-end execution, long-source and physical
two-PC testing remain open, alongside the broader stable v0.4.0 roadmap. No
installed node, release, update feed or version is changed here.
