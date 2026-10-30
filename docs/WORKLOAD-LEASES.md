# Renewable workload lease mechanics

The version-two lease library separates an approved session's maximum lifetime
from its short renewable authority windows. A session contract binds the existing
grant/job/task/attempt, pinned issuer/session/node identities and full-definition
digest, plus explicit session/window limits. Its policy defaults disabled. A
window is at most 30 seconds; the protocol session ceiling is 24 hours and the
default local ceiling is one hour. Existing v1 grants, limits and timing messages
are unchanged.

This is a lease primitive, not a public workload admission implementation. The
internal prepared source uses it, but no startup, registration, command or public
RPC launches a source through it.
The new policy is not yet exposed as an application configuration setting. There
is no new source launch, capture permission, model binding or job database.

The typed reviewed definition derives the contract from a hash of its full
identity, immutable request origin/area, plan, priority, source and limits. Its
initial source shape binds the existing microphone energy adapter by digest,
an exact node-owned resource and private-binding digest, and all effective
sampling and threshold parameters. Private device selection stays in the local
resource snapshot; only its digest enters the shared definition. The local
request builder verifies the actual resource snapshot and adapter bytes before
resolving the private device setting. Missing/null device preserves the existing
owner-configured OS-default selection; it does not attest physical device identity.
The local-only Python request must never be advertised or forwarded.
It requires a bounded owned-process job memory ceiling using the existing memory
validator, returned-source-frame budget and metadata event rate/count/byte/queue
bounds. Event overflow requires stopping, with no unbounded offline fallback.
These are required bounds for future admission, not enforcement by this schema.
Supplying arbitrary digest strings proves no local binding match. Product
admission must use the local check with the resource and bytes it will actually
execute, then retain ownership/check ongoing policy. The memory ceiling is
the existing job-commit limit, not a working-set, CPU or device-buffer quota.
Raw media destinations, extra authority fields, arbitrary plugins and unimplemented
camera/STT/model source shapes are rejected. Nested origin fields are strict for
this new schema without changing older request-context readers. The definition
does not itself issue or consume authority; validation never authorizes capture.
The follow-on [node journal draft](WORKLOAD-RECEIPTS.md) adds internal durable
consumption only. It is not yet wired into coordinator issuance or product
admission, and does not make this definition an execution grant by itself.

The issuer helper retains the original monotonic issuance deadline from an
already-recorded offer; queue delays cannot restart a relative budget. It refuses
an expired deadline or one beyond the contract. It requires the expected authenticated peer identities and exact
contract digest. Only the initial challenge is allowed before consumption; later
windows require a validated consumed-grant acknowledgement. Skipped/replayed
sequences, changed contracts, stopped offers and expired sessions are refused.
The caller must validate that acknowledgement against its existing durable job
journal and the node's authenticated retained report; calling the helper alone
does not establish durable consumption.

The node-side correlator owns fresh nonces, bounded pending probes and local send
times. Replies bind the entire contract and sequence. Its proof cannot be
serialized, deserialized or cloned. Wrong replies cannot complete another probe;
cancelled probes release their pending entries. Two-second probe deadlines, full
round-trip accounting, conservative margins and local clock-change checks bound
the result. Replies carry remaining durations, not timestamps that require
independent machine clocks to agree. Only an authenticated session demultiplexer
may complete probes; that product wiring is still absent.

The supervisor drives one cooperative future across valid renewals. It never
constructs a replacement source or repeats a recording. Renewals cannot move the
first conservative session deadline later. Expiry, a changed scope or an invalid
sequence drops the owned future. A closed renewal channel permits only the last
already-authorized window, then expires. The renewal inbox holds at most four
metadata proofs; it is not an audio/video or event queue. Operation failures are
returned without retry. Dropping a future does not prove OS, device, peer or other
external effects stopped.

Before product use, integrate this on the existing durable job/attempt/receipt
framework. Persist issuance before sending work, consume one exact grant before
ACK/effects, bind the whole reviewed definition and actual resource/model/provider
choices, check local consent and ongoing permissions, and report bounded durable
events. Reconnect/restart must not replay an uncertain session or transfer its
authority to another issuer. Monotonic issuer windows cannot be reconstructed
from a saved wall-clock timestamp as permission to resume work. Resource quotas,
owned-process stop observation, authenticated wire paths, real media and physical
multi-node acceptance remain open.

Focused tests cover policy/version separation, reply scope and nonce correlation,
probe cancellation, clock changes, single-future renewals, queue bounds, expiry,
replay refusal and issuer acknowledgement/stop fencing. These tests exercise the
library mechanics with synthetic operations. Definition tests mutate every
supported scope/source/budget field and verify that each changes the lease hash;
wrong-node resources, missing bounds, unknown nested fields, unsupported sources,
invalid pins and incompatible legacy grants are refused. They do not prove product admission,
network authentication, continuous capture or release readiness.

## Reporting lifetime draft

The consumed runner now binds one in-memory reporting lifetime to its exact
contract. A read-only handle is inactive before the runner starts. Only the lease
supervisor publishes validated windows; a handle cannot renew authority, prepare
a source, mark completion or start a replacement runner. Failed initial binding,
invalid renewal, return and cancellation retire it permanently. Consumption also
retires it before potentially blocked final journal I/O. No lifetime is reconstructed
from retained receipts or restored after restart.

A checkpoint captures the current conservative deadline and sequence. A valid
renewal can update newly created checkpoints, but cannot extend an in-flight
checkpoint. The current window can also tighten an older checkpoint. Checks bind
the full contract digest and refuse expiry or retirement; the original session end
remains fixed. The prepared microphone runner now supervises each useful event
callback under this captured deadline with pre/post lifetime checks. A callback
that retires the runner in its final poll cannot return accepted success.

This is the lifetime boundary for a future bounded blocking journal worker, not
an event persistence implementation. Such a worker must check before and after
its transaction, while its async waiter remains bounded by the captured deadline.
A worker already inside SQLite may outlive waiter cancellation; a late write must
never acknowledge, renew, replay or authorize source work. No reporting-control
mutex may be held across journal I/O. Exact durable event sequencing/deduplication,
storage reservations, downgrade refusal and observed completion remain open.

Eight new regressions cover reporting scope/retirement, captured deadlines across
renewal, failed binding, invalid renewal, final-poll retirement, blocked callbacks
and cancellation while the actual existing journal mutex blocks final persistence.
Existing real-receipt stop/cancellation/result tests also verify handle retirement.
Hosted verification is pending; these are backend/synthetic-source checks, not
physical source, network or release acceptance.
