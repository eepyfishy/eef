# Authenticated workload transport integration

This development checkpoint adds the coordinator's typed connection path for
continuous workload control. It does not complete public execution. The current
production node does not advertise this new support or dispatch an execution
offer, and owner start/stop integration remains unfinished. An existing source
snapshot with `execution_available: false` stays unchanged. No source, model,
permissions, default policy, dashboard or installed node is activated by this work.

## Exact connection and runtime

`NodeServer::open_workload` requires a credential-authenticated TLS connection,
registered version-two workload support, the exact registered runtime/capability,
and definition issuer/session/node matching that connection. The definition must
fit the advertised lease policy. Missing support, a legacy connection, changed
generation, exhausted capacity or a full outgoing queue refuses the offer; no
ordinary operation/RPC fallback or automatic retry is used.

The outgoing `execute_workload` offer contains an exact reviewed definition and
expected node runtime. It carries no lease proof. The caller must already own its
original reviewed issuer and admission/quiescence gates. Opening this transport
cannot create that authority or reconstruct it from retained history.

Inbound frames correlate the grant ID through the actual authenticated node and
connection generation. Each returned frame carries its connection-derived peer
identity separately from the payload and retains byte ownership while the caller
processes it. Another node or retired session cannot confirm or poison the active
owner's route. A link has a separate generation tag so stale caller cleanup cannot
remove a replacement route.

Strict version-two envelopes carry a lease query, consumed ACK, typed metadata
event, terminal completion, or bounded rejection code. Query contract digests,
receipt definitions/flags and consumed timestamps are checked. Events require a
seen consumed ACK, valid typed activity metadata, exact sequence/rate/transition
and reviewed frame/total-byte limits. Arbitrary media/extra fields are refused.
Completion requires the matching consumed ACK and received event count. It does
not independently reconstruct the node journal hash or attest physical capture.
The original issuer must still validate ACK durability, lease sequence/liveness
and completion using the separately supplied authenticated peer identities.

## Bounded processing and retirement

Control and events have separate count bounds. Lease/ACK control bypasses a slow
event consumer. Events use the reviewed `max_pending` allowance including a frame
already dequeued into active processing. Both paths share the existing gateway
serialized-report byte allowance; byte/count permits survive cancellation until
their actual queued/processing frame is dropped. These are serialized metadata
bounds, not a measurement of total process RAM or encrypted wire overhead.

The existing report counter is reused without first allocating another complete
serialized payload. Fixed control envelopes have a bounded size. A malformed or
overflowing owned workload closes that route without closing ordinary node
control. Terminal delivery drains the earlier bounded event prefix; it cannot
overtake and silently discard those events. A terminal link cannot answer another
lease or return more frames.

Caller drop removes only its own link generation. Disconnect removes only that
node/session's routes. Gateway stop retires links before session teardown and
again after routing drains, covering an offer that held routing read ownership
when stop began. A response through a retired link is refused. The actual driver
must retire its original issuer and report uncertainty on these failures; transport
teardown by itself is not an observation that a physical source stopped.

## Verification boundary and remaining integration

Ten added regressions cover runtime/scope/schema binding, missing/legacy support,
actual peer versus payload identity, active event count and global byte ownership,
control bypass and terminal ordering, malformed/duplicate/unreviewed data,
replacement-session cleanup, offer queue failure/disconnect/gateway stop, shared
control/event byte pressure, and a real pinned-TLS credential-authenticated
loopback challenge with bounded ACK/event/completion delivery.

The loopback peer simulates receipts. It checks framed transport and the original
lease challenge, not owner job approval, durable node consumption, source effects
or stop completion. The production node dispatcher, bounded consumption admission,
renewal pump, event callback/terminal handling, owner runtime and CLI start/stop
commands still must be connected and validated together. Only then may actual
execution support be advertised. No camera/audio relay, physical two-PC or stable
v0.4.0 acceptance is implied. Heavy local builds/runtime tests are not used; full
changed-source hosted and final main-ref acceptance remain required.
