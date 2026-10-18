# Peer transport security foundation (unreleased)

This is an experimental library in `eefn::peer`, not a usable node-to-node
feature. No app command, listener, capability advertisement, permission,
execution-grant operation or media route enables it. It is the transport
foundation for step 8 of the [command-first roadmap](ROADMAP-CORE.md), not
completion of that step or of [presence and media](PRESENCE-AND-MEDIA.md).

## Authority and credential privacy

The existing approved jobs and consumed execution grants must remain the only
work authority. Validating a peer scope, sealing credentials, decrypting a
bundle or completing TLS does not approve a job or consume a grant. There is no
second peer approval system in this increment.

A scope binds both nodes, their control sessions and runtime generations,
distinct grant/resource identities, coordinator identity/runtime, immutable
request context, job and plan digest, one destination address, direction,
lifetime and transfer limits. Strict shape/size checks reject incompatible
metadata; they do not establish current permission, resource availability,
disclosure or approval.

Inspectable job/plan/grant parameters must never expose raw TLS private keys.
An origin node inspecting a job involving two other nodes must not gain either
endpoint's private material. The preparation function therefore creates fresh
per-pair CA, leaf keys and framing key, and returns two encrypted bundles:

- Each bundle uses only its recipient's independent, unrevoked node credential,
  with a separate HKDF domain and authenticated role/node/grant/scope binding.
  A shared network PSK cannot substitute for a node credential.
- The pinned coordinator signs the complete encrypted envelope. Possession of
  a recipient's decryption credential alone cannot forge coordinator issuance.
- Opening a bundle checks the observed local pairing, control session, runtime
  generation, coordinator epoch, scope digest, signature and time before making
  an endpoint. The observed identities must eventually come from the running
  grant-owned service, never model output or untrusted discovery.
- Raw private material is private to the module and omitted from Debug reports.
  Ciphertext delivery is serializable; this is not a claim of memory zeroization.

Signing currently supports the existing generated P-256 coordinator identity.
An unsupported custom coordinator signing key fails, without unsigned fallback
or silently replacing the owner's identity. `ring`, `time` and `x509-parser`
are direct dependencies at already locked/cached versions; no model or toolchain
download is part of this increment.

## Authenticated stream and failure behavior

The source initiates and destination accepts an IO stream supplied by the
caller. The library does not dial or bind a socket. Future runtime admission
must enforce the approved address, listener ownership and number of attempts.

TLS 1.3 requires both scoped endpoint certificates, the fresh pair CA and the
exact expected peer leaf. Its ALPN is separate from the coordinator protocol.
Early data, resumption and server session retention are disabled. Wrong peer,
another pair, plaintext, missing client certificate and control-protocol ALPN
are rejected. Payload framing reuses the existing encrypted newline protocol
with a fresh pair key, strict scope/role/sequence and bounded decoding.

Only the source sends data; only the destination acknowledges it. Outstanding
frame metadata is bounded. Each acknowledgement must match an exact cumulative
sent prefix, not just a plausible larger counter. It is endpoint-reported
receipt evidence, not physical playback, job completion, external side effects
or proof that effects stopped.

Protocol errors, incomplete framing, IO errors, exhausted byte budget, expiry
and observed clock discontinuity fence the channel. Cancelling an in-flight
read/write also fences it: a partial frame must not be retried, resumed or
reinterpreted as a new sequence. Repairing a clock does not revive a fenced
channel. Pre-IO argument/window rejection does not consume data: a source may
drain a valid acknowledgement before attempting more data within its grant.

The endpoint's original monotonic deadline is retained across handshake and
IO; it is never renewed by traffic. A waiting read wakes at expiry even if the
peer sends nothing. This library is not a background connection-reaping task;
an inactive owner must still drop its channel on stop/revocation/disconnect.

## Bounds and their exact meaning

Every scope supplies its own limits within these validation ceilings, not
promised performance or application configuration defaults:

| Bound | Ceiling / meaning |
| --- | --- |
| Lifetime | Existing execution-grant ceiling, currently 30 seconds |
| Combined bytes | 64 MiB of TLS **application** bytes, both directions, including encrypted framing/control overhead |
| Rate | 64 MiB/s maximum, token bucket with one second of burst; both directions share one channel budget |
| Payload frame | 256 KiB |
| Data frames | 65,536 |
| Unacknowledged frames | 8; frame-size times window at most 2 MiB |
| Read allocation | Bounded frame line, 8-KiB reader and 16-KiB IO scratch, plus TLS/socket buffers |

Byte/rate accounting is not physical packet accounting: TLS handshake, TLS
record headers and underlying network overhead are excluded. These are
per-channel limits, not aggregate node quotas or hard OS memory/CPU limits.
No unbounded offline queue or automatic retransmission is supplied.

## Verification and remaining product integration

Focused tests exercise recipient isolation, coordinator signature forgery,
scope substitution, old generations, mutual TLS, direction/framing/sequence,
exact acknowledgement prefixes, bounded flow control, application-byte rate,
expiry and fatal fencing. Partial-IO cancellation tests require actual accepted
or consumed bytes before dropping the future. A loopback TCP test transfers
synthetic bytes; another observes the underlying TLS wire and checks that the
plaintext and its base64 representation are absent. These are local synthetic
tests, not physical two-PC or live audio/video acceptance.

Both final locked/offline workspace/all-target suites pass **789 tests plus one
ignored each**, including no-dashboard: EEF 247 plus one ignored opt-in profiler,
owner CLI 17, installer 6, EEFN 501 and node CLI 18. The EEFN suite includes 21
new peer tests. The backend/browser source guard now checks nested runtime
modules as well as top-level services; it passes without a UI dependency.

Fresh headless binaries/examples build. Public `local-pipelines-TX38Bw` and
`model-downloads-MB5qua` pass the existing command chains, including durable
progress/install history, permission loss, restart and no-replay cases. Both
fixtures are terminal with no matching fixture EEF/EEFN/native-helper process
left. Browser smoke also passes initialization, copying/selection stability and
truthful installation status; no browser source was changed. These regressions
show the existing services remain usable, not that a public peer path exists.

Evaluated headless binary SHA-256:

```text
eef.exe  2b55c82fce8bde4f2dbe0a70ef1aeecee9dabf3d1bfc05d0ec0ac896b58d5bea
eefn.exe 619b0dda63ecde44b69aaab13b33ec59f15c2845c99106753a1ee3e2ede85db2
```

Before exposing a usable node session, integrate and verify all of:

1. Existing reviewed jobs, paired execution grants and durable consumption before
   effects; one attempt cannot reopen/replay a bundle after restart/reconnect.
   `open` is stateless and **does not provide** durable replay admission.
2. Fresh control identities, permission/resource/disclosure checks, destination
   reachability, exact listener/dial ownership and bounded simultaneous sessions.
3. Existing command/report services for issuance, acceptance, progress and
   completion; no dashboard dependency or raw private material in inspection.
4. Stop/revocation/permission loss/disconnect handling, strong observed outcomes,
   aggregate resource quotas and responsive control under competing transfers.
5. Explicit bounded relay policy, actual media adapters and real two-PC packet,
   resource, privacy and failure acceptance before claiming those features.

In particular, TLS/frame replay rejection inside one channel is not evidence
that application session replay or stale coordinator leadership is solved.
No installed/remote node, update policy, model selection or release changes.
