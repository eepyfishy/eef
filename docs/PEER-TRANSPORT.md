# Peer transport security foundation (unreleased)

Experimental `eefn::peer` now includes an advanced opt-in finite audio path in
native app startup. All permissions remain disabled by default. The earlier
static-buffer foundation and its historical checkpoints are retained below;
their startup-disabled descriptions apply to those checkpoints, not this path.
This advances step 8 of the [command-first roadmap](ROADMAP-CORE.md), not
completion of that step or of [presence and media](PRESENCE-AND-MEDIA.md).

## Finite capture to playback (unreleased, advanced configuration)

An explicit `peer_transfer` request can now describe schema-version 2 audio:
`audio` binds `duration_ms`, `sample_rate`, `channels`, `source_resource_id` and
`destination_resource_id`. The existing source/destination transport resources
and limits remain required. `payload_bytes` is the format-derived maximum;
`payload_sha256` is omitted because recording must not happen before approval.
Legacy schema-version 1 static transfers retain their exact serialized shape
and preknown digest. Neither content mode silently falls back to the other.

The source and destination advertise `finite_audio` only through their existing
authenticated, journal-capable control session. Explicit reviewed submission,
separate owner approval and execution are required; this template is not added
to the LM method vocabulary or reusable automation. Review includes all three
operations: `peer.transfer`, `audio.capture` and `audio.play`. Existing directional
disclosure rules still apply. Both grants and recipient-encrypted delivery
materials are saved atomically. Destination readiness must be durably observed
before dispatching the source. Both nodes consume their existing grants before
effects; repeated execution or reconnect never replays them.

Owner setup now has [typed local/EEF commands](RELAY-SETUP.md), including missing
resource declarations and OS-selected receiver ports; complete non-coder media
acceptance is not claimed. The underlying configuration still needs
`permissions.peer_transfers.enabled` and an available
`peer.transfer` transport resource, and enable the appropriate microphone/output
switch and installed Python adapter. The source needs a physical `audio.capture`
resource, optionally with its `device` binding; the destination needs an
`audio.play` resource with no extra parameters. The existing Windows playback
adapter uses the default output, not a selectable speaker or virtual call input.
An optional numeric `peer_destination_address` selects the receiving address;
null is sender-only. No guessed port, wildcard bind or startup listener is
created. Configuration uses the existing service/remote approval. Applied route
changes require restart; changing the saved receive route fences active audio
rather than silently moving it. Status distinguishes configured transport from
current session advertisement and clears the latter on disconnect.

Only 100–10,000 ms PCM16 WAV is supported, with one or two channels and rates
8,000/16,000/24,000/32,000/44,100/48,000 Hz. Byte bounds allow one frame of existing
adapter rounding and at most 1 KiB RIFF metadata. Capture is validated and
canonicalized, stripping incidental metadata. A bounded authenticated manifest
then binds the actual count/digest before PCM transfer. The receiver validates
the complete canonical WAV before calling the approved sink. Its final data
acknowledgement is withheld until playback returns. Both successful endpoint
reports must agree on actual bytes/digest; no raw audio enters coordinator
inspection or retained job reports. `producer_reported` / `sink_reported` are
endpoint observations, never physical-effect proof; `sink_effects_confirmed`
and `payload_retained` remain false.

Derived media operations recheck applied and saved permissions/resources under
the parent grant watcher. Installing an owner plugin cannot bypass a disabled
known media switch. Existing bundle admission counts one executor future per
endpoint and also reserves the exact microphone/speaker provider dependency.
Bounded bridge/transport deadlines, quota checks and uncertain consumed outcomes
remain in force. This does not claim Python heap/CPU quotas, strong physical
stop, continuous streaming, a virtual microphone/mixer, camera relay, presence
tracking, physical two-PC acceptance, or partition-safe coordinator leadership.

Development acceptance uses fresh headless binaries/examples and an explicitly
selected existing trusted interpreter, without downloading one:

```powershell
$env:EEF_TEST_PYTHON = 'C:\path\to\trusted\python.exe'
node tools/test-local-pipelines.mjs
node tools/test-python-bridge.mjs
```

The optional Python pipeline fixture isolates built-in device adapters and uses
synthetic microphone/speaker plugins through the real loader, permission checks,
public review/approval/execution services, consumed journals and native mutual
TLS. It checks no effects on save/approval, matching endpoint receipts,
permission/route loss, malformed capture, provider accounting, replay refusal
and fresh work after failure. It opens no real camera/microphone/speaker and is
not a substitute for installed-node or physical two-PC acceptance. The regular
pipeline checks remain available without the interpreter environment variable.

## Authority and credential privacy

The existing approved jobs and consumed execution grants must remain the only
work authority. Validating a peer scope, sealing credentials, decrypting a
bundle or completing TLS does not approve a job or consume a grant. There is no
second peer approval system in this increment.

A scope binds both nodes, their control sessions and runtime generations,
distinct grant IDs and full grant digests/resource identities, coordinator identity/runtime, immutable
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
  an endpoint. The observed identities come from the running
  grant-owned service, never model output or untrusted discovery.
- Raw private material is private to the module and omitted from Debug reports.
  Ciphertext delivery is serializable; this is not a claim of memory zeroization.

Signing currently supports the existing generated P-256 coordinator identity.
An unsupported custom coordinator signing key fails, without unsigned fallback
or silently replacing the owner's identity. `ring`, `time` and `x509-parser`
are direct dependencies at already locked/cached versions; no model or toolchain
download is part of this increment.

## Authenticated stream and failure behavior

The underlying channel accepts caller-supplied IO. Native consumed admission
also offers `run_tcp`: it owns one numeric scoped bind/accept or connect, with
socket waits and readiness output under the existing lease/policy watcher.
See the native-delivery section below for its boundaries.

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

## Consumed finite transport admission (unreleased follow-up)

`peer::admission` adds a finite, transport-only `peer.transfer` grant contract
(`send`/`receive`). Its immutable definition names the other node/resource,
exact payload length and SHA-256, and transfer limits. It contains no credential,
runtime, address or plan hash. Ephemeral signed/encrypted delivery remains
separate, so an approved plan never needs to hash itself or expose raw keys.
Both parties in the signed scope now include their full execution-grant digest.
These are unreleased library types, not an existing application wire migration.

`AdmittedPeer::prepare` checks the definition, exact grant and recipient delivery
(including issuer, job, plan, request origin, node/session/generation, resource,
limits and time containment), then uses the **existing** `GrantLease` and
`AcceptedGrant`/`GrantLedger` admission. There is no second receipt database,
deserializable lease or production wall-clock admission fallback. Signature,
scope, policy, resource, local capacity or journal failure cannot return an
admitted endpoint. Ordinary grant preparation and `NodeEngine::execute` refuse
this operation rather than silently discard its delivery requirements.

An admitted object owns its consumed grant and transport slot together. It
offers no raw channel extraction, reconnect or restart continuation. Finite IO
uses the same grant deadline, permission/resource watcher, cancellation-to-unknown
and durable final-report path as existing operations. A shorter fresh issuer
lease terminates a stalled handshake even if the signed scope lasts longer.
Peer TLS/scope validation still requires a suitable local clock; this is not a
claim of clock-independent peer certificate acceptance.

The experimental engine admission flag is **not enabled by app startup** and
does not add a capability advertisement, even for a library caller enabling it.
An additive `permissions.peer_transfers` policy defaults disabled. It bounds
payload, application wire bytes, rate and concurrent admitted sessions per engine
(default one, ceiling eight). Definitions have a 4-MiB payload ceiling; receive
processing hashes/discards one bounded frame at a time. These limits are not
machine-wide CPU/memory quotas or a shared aggregate bandwidth scheduler.

The finite executor checks the source manifest before TLS IO and checks the
received layout/hash before acknowledging its final frame. Its final report
retains only scope/session, role, digest and counts, with explicit
`payload_retained: false` and `sink_effects_confirmed: false`. It does **not**
capture a camera, read/write a file, play audio or deliver to an application.
Such producers/sinks require their own approved resource/permission integration.
The sender's receipt alone cannot establish durable receiver completion.

Existing consumed-attempt uniqueness and grant-ID checks refuse reuse after
completion, drop, cancellation or reopening the same journal. Raw `open` remains
stateless and must not become a production bypass. This admission primitive is
not evidence by itself that the coordinator approved or supervised both sides.
The following coordinator increment adds paired planning/issuance; the subsequent
native executor adds opt-in finite delivery, not app producer/sink setup. Ordinary
reviewed jobs retain their predecessor and one-active-attempt rules.

## Coordinator paired jobs (unreleased; native delivery still unavailable)

The explicit `peer_transfer` input now describes two fixed node/resource
endpoints and an exact finite manifest/limits. Each endpoint definition includes
its own `resource_id`, consistent with existing resource binding conventions.
The template is not an inferred language-model method. Root constraints cannot
override either endpoint, and route addresses, epochs and credentials are not
accepted inside the immutable input.

Compilation uses existing origin review and planning records; only the existing
coordinator owner can approve it. Exactly two tasks (`send`, `receive`) have no
predecessor dependency, no retry and fixed distinct node/resource bindings.
This is a special bounded pair, not general concurrent reviewed execution:
ordinary templates retain their predecessor and one-active-attempt invariants.
Reusable automation authorization for pairs is refused for now.

The driver revalidates providers, issuer, fresh credential/TLS grant sessions,
node bounds and registration-generation-bound peer support, then reserves both
providers before minting. Runtime destination address and node generations come
from those accepted advertisements, not model output. One existing journal
transaction saves **both** grants, full digests, the scope, public coordinator
certificate and recipient-encrypted/signed deliveries before either dispatch.
Quota/signature/binding failure leaves both tasks unissued, with no partial
checkpoint. Private keys or bulk payloads are not added to the job record.

Destination dispatch precedes source dispatch. A readiness observer releases
the source only after the destination's exact ACK has been saved by the existing
journal observer. The native handler binds and retains its authorized
route before emitting that ACK. Rejection, timeout, stop or changed scope before
readiness sends no source request; it does not fabricate a source-node receipt.
The new control envelope reuses existing framing, leases and report queues;
ordinary `execute_grant` is not a delivery fallback.

Each endpoint's result must confirm its own grant **and the saved pair session
and scope hash**. One endpoint cannot complete the parent. While its counterpart
is active, an individual failure remains visible but the parent stays running
or stopping. Once both are terminal, any unknown result yields interrupted;
otherwise failure yields failed, and two matching successful transport reports
can complete the transport job. This still proves no application sink effects.

Restart preserves the exact pair and changes unfinished attempts to unknown,
without renewal, replay, rebinding or migration. Saved envelope signatures are
validated at issuance time, so inspection does not become invalid merely because
the short scope or signing certificate later expires; this renews no authority.
Partial paired reconciliation and one-grant tracking abandonment are explicitly
refused until joint uncertainty handling is implemented. Exact read-only receipt
inspection remains available. Older development coordinators lacking the typed
optional execution field cannot safely inspect these pair records: do not
downgrade a journal containing them. Ordinary records keep their prior shape.

Compatible nodes must declare versioned peer support tied to their accepted
registration runtime. Missing/old/changed support is refused before dispatch.
**Current native node startup still declares no such support** and enables no
peer admission. There is therefore no usable installed-node peer command yet.
The opt-in native executor is described below; app producers/sinks, strong stop, joint recovery,
bandwidth supervision and physical acceptance are not complete.

Seven focused tests cover structural bindings, atomic/failed journal writes,
joint completion, cross-session results, rejection/unknown and restart. One
drives the actual coordinator over authenticated TLS control sockets, checking
both-grants-before-dispatch and ACK/result ordering with **emulated executors**.
It does not move peer payloads, consume real executor journals or substitute for
native public-command or physical two-PC acceptance.

Both final locked/offline workspace/all-target modes pass **808 tests plus one
ignored each**: EEF 254 plus one ignored opt-in profiler, owner CLI 17, installer
6, EEFN 511 and node CLI 20. Fresh headless binaries/examples build. Existing
public `local-pipelines-BVbdaV`, `model-downloads-OAapCW`,
`node-credentials-KuhF7n` and `request-drafts-zsRVWd` pass command, private
credential, history, restart and no-replay regressions. Browser initialization,
copy stability/status and the recursive backend/browser source guard pass.
The fixture processes are terminal; no installed/remote node or release changed.

Paired-coordinator checkpoint headless binary SHA-256:

```text
eef.exe  a359a9446b921cfc48e862615844ad7ca26fefc05b53d07f6a995791f585576f
eefn.exe 37f1f53847d727beb288a261f52c1d140400877f0b01d7f0af11cd7c18b3dd1a
```

## Native finite delivery executor (unreleased; app startup remains disabled)

`NodeClient::with_peer_runtime` accepts an explicitly constructed `FiniteRuntime`
only with applied peer permission, engine admission and existing available local
`peer.transfer` resources. It enables no permission itself. The embedding owner
supplies immutable in-memory source snapshots or verification-only receive
resources and one numeric local receive address. Bindings are bounded to eight
resources and 4 MiB total source snapshots; admitted calls share immutable byte
storage rather than copying a payload per pending lease. Payloads are neither
serializable nor included in Debug, registration, status or receipts. There is no
file access, capture, playback, dynamic producer or application sink in this hook.
The owner's original source snapshot lives with its runtime; the receipt's
`payload_retained: false` means no received/report/job payload retention, not
destruction or memory zeroization of that separately supplied source.

An opt-in client advertises only with independent credential/pinned TLS,
available consumed journal, immutable node generation/registration offer and a
valid coordinator epoch from the authenticated welcome. Old coordinators remain
compatible but cannot activate peer support. Unbound peer resources are marked
unavailable. The gateway's fresh epoch is also the coordinator Runtime's epoch;
delivery cannot supply or override the observed identity. This is process/session
binding, not replicated-state election or stale-leader fencing across a cluster.

The strict `execute_peer_grant` envelope uses existing request capacity, fresh
issuer lease, consumed ledger, exact recipient-bound admission, control output
failure guard and connection-owned tasks. Source manifest and local receiver
resource/address must match before consumption. No fallback to ordinary RPC,
ordinary execution or stateless channel opening is permitted.

After consumption, `run_tcp` binds the destination route before readiness ACK;
source opens one connection only after its own ACK. Binding, readiness writes,
accept/connect, TLS and framed transfer share the original lease and relevant
permission watcher. No retry/reconnect loop is added. Completion, cancellation
or control-session loss drops owned socket futures; consumed authority remains
completed or unknown and cannot be replayed. A caller stopping its coordinator
RPC is not a strong remote stop: connected node work may remain until its lease.

If a consumed destination cannot bind, it reports persisted `Unknown` without a
readiness ACK, never `Rejected`. The gateway accepts that conservative report
only for peer authority after an issued lease probe. The paired journal can save
it without releasing source dispatch. Successful completion still needs the
matching saved ACK; ordinary grant lifecycle rules are unchanged. Parent and
counterpart interruption still use the existing coordinator driver/guard.

Native regression cases exercise real NodeClients, their consumed journals, the
coordinator paired driver and actual loopback peer TCP/mutual TLS. These are
finite synthetic producers, not public installed-node commands, physical two-PC
acceptance, media delivery or proof of application effects. The remaining product
requirements below still apply.

Final verification: both locked/offline all-target feature modes pass **815
tests plus one ignored each**. This includes three native coordinator/client
cases, two socket ownership/deadline/cancellation cases and two bounded runtime
binding cases added in this increment. Ordinary pre-ACK uncertainty remains
refused in its existing lifecycle regression. The native disconnect check reads
SQLite read-only, so opening/reconciling the journal cannot manufacture Unknown.
Fresh no-dashboard binaries/examples build. Public
`local-pipelines-KQuE4y`, `model-downloads-KGVT1P`,
`node-credentials-rwuVyQ` and `request-drafts-Z8puop` pass existing app flows;
they do not activate the new producer bindings. Browser copy/status smoke and
the recursive backend/browser guard pass. No UI implementation changed.

Evaluated headless binary SHA-256:

```text
eef.exe  45ea34b693d23d0584187f282e2b8755bb515f9cd2df9a18c4c908630ea96cdf
eefn.exe 16f1c0bd9db27953b098f5dd02d9f5e30e74fa94f5c5c5eb8a9d947dea2b1a34
```

## Verification and remaining product integration

Focused tests exercise recipient isolation, coordinator signature forgery,
scope substitution, old generations, mutual TLS, direction/framing/sequence,
exact acknowledgement prefixes, bounded flow control, application-byte rate,
expiry and fatal fencing. Partial-IO cancellation tests require actual accepted
or consumed bytes before dropping the future. A loopback TCP test transfers
synthetic bytes; another observes the underlying TLS wire and checks that the
plaintext and its base64 representation are absent. These are local synthetic
tests, not physical two-PC or live audio/video acceptance.

The transport-foundation checkpoint passed **789 tests plus one ignored each**
in both locked/offline workspace/all-target modes, including no-dashboard:
EEF 247 plus one ignored opt-in profiler,
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

Foundation-checkpoint headless binary SHA-256 (before the admission follow-up):

```text
eef.exe  2b55c82fce8bde4f2dbe0a70ef1aeecee9dabf3d1bfc05d0ec0ac896b58d5bea
eefn.exe 619b0dda63ecde44b69aaab13b33ec59f15c2845c99106753a1ee3e2ede85db2
```

The admission follow-up adds ten focused tests: exact consumed-grant TCP
transfer and retained metrics without payload storage, ordinary-path refusal,
scope/signature/manifest/lease substitution, dropped/reopened journal replay,
shorter issuer lease, cancellation, saved permission loss, local quota/resource
denial, full journal without eviction, and source/receiver payload mismatch.
Its lease demultiplexer is synthetic; do not treat it as public paired-command
or physical-network approval evidence.

The initial fresh-binary regression exposed a separate native startup/pairing
failure (`local-pipelines-JcnMLR` and `fs8can`). It was not accepted as a pass.
LLDB located the stack overflow in Clap command-tree construction on the
1-MiB Windows main stack; type measurements showed dispatch/pairing/runtime
futures of 5,376/3,784/8,896 bytes, not an enormous async future. Argument parsing
now happens synchronously before Tokio entry, retaining selected-version handoff
before parsing. A regression constructs representative command trees on a
1-MiB thread; the evaluated executable still reserves 1,048,576 stack bytes.
Fresh-binary public pairing/command regressions below pass after this fix.

Both final locked/offline workspace/all-target modes pass **801 tests plus one
ignored each**: EEF 247 plus one ignored opt-in profiler, owner CLI 17, installer
6, EEFN 511 and node CLI 20. Fresh headless binaries/examples build. Public
`local-pipelines-c8jcbY`, `model-downloads-1dj8ED` and
`node-credentials-m62kSj` pass existing pipeline/progress/permission/restart,
installation history/no-replay, independent credential/pinning/rotation/revocation
and private pairing commands. No matching fixture app/helper process remains.
These are existing public regressions, not public peer-session support.

The first browser rerun read the hidden Advanced textarea before its hash-change
handler refreshed it. The smoke test now waits for the Advanced page's visible
state before reading, preserving every existing assertion and application file.
Two final browser runs pass initialization, copy/selection stability and truthful
installation status; the recursive backend/browser source guard also passes.

Admission-follow-up evaluated headless binary SHA-256:

```text
eef.exe  529c84baa973a43ba6c182857c92c208b01f329b197ce5b6478c3dab0a40df75
eefn.exe 25d0e2e38deec3ed2d786b56f0ec618d696f71fccd4e6c70b6d7f6788c94badc
```

Before exposing a usable node session, integrate and verify all of:

1. Existing reviewed jobs, paired execution grants and durable consumption before
   effects; integrate the consumed-admission primitive into actual command
   dispatch, and never bypass it with stateless `open` after restart/reconnect.
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
