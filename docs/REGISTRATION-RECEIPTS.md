# Correlated registration receipts (unreleased)

Node status now distinguishes a registration write from EEF acknowledging that
registration on the current authenticated session. `eefn setup`, node status and
minimal diagnostics expose `registration` as `accepted`, `awaiting_receipt`,
`unknown`, or `not_connected`. Setup text explains registration separately from
the local input queue, model/interpreter configuration and reviewed-work transport.
None of these observations grants permission or submits work.

## Negotiated, bounded framing

The existing encrypted authentication reply can advertise
`registration_receipts: {schema_version: 1}`. Only then does a runtime-bound node
include a strict `registration_receipt` offer in its normal register message:
schema 1, the current node runtime UUID and a fresh random nonce for that TCP
connection. A reconnect gets a new nonce even if the runtime UUID is unchanged.
Model/inventory refreshes retain the same context during that connection.

Older coordinators without the capability receive the original registration
shape. Older nodes that do not offer receipts receive no extra receipt frame.
Omitted registration fields in older setup reports remain unknown, never assumed
accepted. There is no compatibility downgrade, alternate enrollment, additional
listener or model dependency. Library clients must explicitly bind their runtime
with `with_registration_runtime`; a mutable status object is not used to invent
an identity binding. The actual node app supplies its existing runtime UUID.

After all normal registration validation and storage for that active gateway
session, EEF queues a `registration_accepted` control frame. It contains schema,
exact authenticated node ID, offered runtime UUID/nonce, and the gateway session
UUID from authentication. It uses the existing bounded outgoing queue/framing;
queue failure retires the session instead of creating another queue or replaying
work. Registration context cannot change or disappear on an in-session refresh.

The node validates the exact fields, current offer, node ID and authenticated
session before recording the receipt. Unsolicited, malformed or mismatched
receipt frames retire the connection; the client does not reinterpret them as
success or execution approval. Receipt data lives in internal runtime status,
not a new durable receipt journal. Node status/diagnostic/setup observations expose
the state, not the raw receipt. Registration events and gateway snapshots can
still carry the offered public correlation IDs; these are not secrets or grants.

## Truthful lifetime and scope

`accepted` requires a valid receipt, matching current node runtime, a connection
still marked connected, and an open local session receiver. Closing that receiver
demotes a historical Connected label instead of preserving accepted readiness.
Disconnect/check/reconnect status changes clear the old receipt, and a new runtime
cannot reuse it. The existing EOF path already retires the connection as an error;
the new regression test verifies receipt clearing on that path, not a new EOF
success behavior. Input-capacity hints keep their established ordering in setup.

This is a snapshot, **not a liveness lease**. It does not prove future delivery,
latest model-refresh acknowledgement, scheduler/provider ingestion, inference,
hardware identity, OS-user identity, media health, capacity availability or work
authorization. A busy queue may coexist with accepted registration. A configured
interpreter is not proven healthy, and a model-free node can be registered.
Shared-key transport does not gain independent-node identity guarantees just
because its peer replies; pinned private TLS and execution grants remain separate.

The nonce/session checks prevent accepting a receipt for another connection or
runtime. They do not add durable exactly-once work semantics, distributed election,
leader fencing, arbitrary application replay protection or rollback/undo.
Observation never retries input or a submitted work operation. Modern nodes
without a receipt must not be presented as verified solely because TCP connected
or the local register write finished.

Private local enrollment still reports its original mutation checkpoint—saved
pairing and a requested restart—not completed registration. Clients can inspect
the subsequent backend status/setup receipt without becoming the controller.
Full no-coder graphical onboarding, default interpreter/bootstrap, physical
two-PC, continuous media, peer/mesh/cluster and installer/signing/release acceptance
remain separate requirements.

New readers accept missing registration observations in older reports as unknown.
Older strict readers may need a matching upgrade to accept the additional setup
field; they must not ignore validation failures or invent acceptance as a fallback.

## Verification checkpoint

Final workspace/all-target suites pass with default features and separately with
`--no-default-features`: **698 passed, one ignored in each configuration** (EEF
library 243, EEF CLI 17, installer 6, node library 417, node CLI 15). Fresh default
binaries/examples build; formatting, diff, JavaScript syntax and the backend/UI
dependency source check pass. The initial EOF-test success expectation was
corrected to the existing disconnect error behavior. A full-suite regression in
busy-queue hint ordering was fixed while preserving the prior input-capacity
ordering; the final suites include that existing test.

Protocol/unit checks cover fresh nonce/context validation, mismatched node,
runtime/session and extra authority fields, retired runtime/receiver suppression,
real encrypted gateway acknowledgement/refresh, rejection of changed in-session
context, and the existing EOF retirement path. A library client with a bound
runtime observes acceptance from a real local gateway, then loses it after stop.
Setup checks preserve unknown older reports and do not infer model/work readiness.

Fresh headless integration evidence:

- `.validation/local-enrollment-JTFOC0/results.json`: two logical nodes privately
  enroll and subsequently observe receipts, without inference or permission grants.
- `.validation/setup-check-wU0inv/results.json`: setup/plain/JSON and older missing
  fields remain truthful; invalid registration state is refused without echoing it.
- `.validation/node-credentials-xMOA8V/results.json`: existing private pairing,
  revocation, model-free reviewed work and transport regressions pass.
- `.validation/node-output-BgCWVk/results.json`: a legacy peer without negotiation
  receives the original register shape and stays `unknown`; blocked output retires
  and reconnects with fresh input ownership and no response/work replay. This is
  legacy encrypted loopback, not pinned TLS or a native termination/physical test.
- `.validation/owner-boundary-zjNg1g/results.json`: the prior real-socket owner HTTP
  boundary regression still passes; it does not test framed node authorization.

These are local same-PC checks, not physical two-PC or complete release evidence.
No installed/remote app, release version/asset, update selector or model is changed.
Test app/compiler processes exit after verification; user `docs/future/` is untouched.
