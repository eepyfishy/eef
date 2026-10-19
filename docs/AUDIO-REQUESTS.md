# Finite audio requests without JSON (unreleased)

After [relay setup](RELAY-SETUP.md), a running node can prepare a finite
microphone-to-speaker request without an LM, JSON file, port numbers or resource
UUIDs:

    eefn requests submit-audio --to DESTINATION_NODE_ID --duration-ms 2000

The source defaults to this authenticated origin node. The destination is an
exact node ID listed by network peers, not a guessed hostname. For another source:

    eefn requests submit-audio --source-node SOURCE_NODE_ID --to DESTINATION_NODE_ID --duration-ms 500

Both endpoints must be distinct, currently registered, support finite peer audio
and be visible through the origin's existing directional disclosure policy.
Only self is visible by default; the coordinator owner must explicitly disclose
other nodes. Disclosure is not work permission. Authoring never pairs nodes,
changes discovery/permissions, downloads/loads a model, opens a microphone,
plays audio, approves a plan or requests execution.

The plain result shows actual source/destination, format, job ID/status and review
commands. The JSON flag provides the same typed backend report for clients.
Separate coordinator-owner review, approval and explicit execution still use
the existing [request workflow](EXPLICIT-REQUESTS.md). Authoring success means
saved/compiled, not working audio or completed work. Published alpha.6 installers
do not contain this command.

## Resolution and immutable receipts

The coordinator uses fresh pinned control registrations under the same
origin-scoped disclosure policy as existing planning. It chooses exactly one
available transport and media resource per requested endpoint. It never ranks
ambiguous microphones/speakers or falls back to another node. Advanced selectors
are source-transport, destination-transport, microphone and speaker flags, with
local or matching fully qualified resource IDs. Disabled, unavailable,
wrong-owner, unsupported and ambiguous choices are refused.

The original typed intent and resolved concrete pair are saved together in the
existing review/job journal before compilation can complete. No separate audio
approval system or runner is introduced. Journal checkpoints cannot replace
selected resources, typed intent or immutable origin, including before approval.
Legacy records without typed audio intent keep their old serialized shape/hashes.

An operation ID is created automatically; retain the entire command/report after
uncertainty. The operation-id flag can explicitly reuse that exact receipt:

    eefn jobs find --operation-id UUID
    eefn requests plan JOB_ID

A retained source is matched before fresh endpoint lookup. Its intent keeps the
original resource choices even after resources change, the coordinator restarts,
or disclosure is revoked. Retained compiled history is inspected rather than
resolved again. A retained review whose compilation never completed may only
compile against those same saved choices, never select replacements or authorize
work. Conflicting intent under the same operation ID is refused. Explicit
removal of history removes that evidence; a missing receipt does not prove that
no earlier request existed. These receipts are scoped to the coordinator's
retained journal: another coordinator with an unreplicated database cannot prove
that a request was never accepted elsewhere. Replicated cluster history and
leader fencing remain unfinished; do not automatically resubmit across stores.

Dropped, corrupted and unsupported replies cause no automatic retries, replacement
IDs, generic job/chat submission, inference or permission fallback. Inspect history
before deciding what to do. To intentionally choose a newly configured resource,
submit a new operation, review its actual choices and separately approve it.
This is not resumption or replay of an old grant.

## Format, bounds and remaining acceptance

Defaults are 2,000 ms, 16,000 Hz, mono PCM16 WAV. Duration-ms, sample-rate and
channels flags request a [supported finite format](PEER-TRANSPORT.md); duration
is 100–10,000 ms. Requested transport bounds remain subject to local node
byte/rate/session policy, Python bridge limits and execution grants. Advanced
max-wire-bytes and max-bytes-per-second flags never widen node limits automatically.
Preparation is not a reservation or proof that saved/applied permission, route
and hardware checks will pass at execution.

Node clients use /api/commands/requests/audio, the existing correlated submission
channel and requests.audio coordinator handler. Callers supply typed intent, not
an authoritative resource snapshot or forged origin. Exact original intent,
concrete definitions and retained review proof are validated before success is
acknowledged. No dashboard is a controller.

This is finite PC audio only. Continuous streaming, virtual call inputs/mixing,
physical device switching, presence/camera selection, default model bootstrap,
real two-PC hardware/termination/resource tests, installer/signing/Defender and
stable v0.4.0 acceptance remain unfinished.
