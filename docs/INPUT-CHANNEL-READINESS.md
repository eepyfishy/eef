# Local input-channel readiness (unreleased)

The existing node client now installs its local submission receiver before
publishing `connection.state: connected`. Authentication, registration framing,
reachability probing, reconnect backoff, permissions and grant checks are unchanged.
This closes the source-order window documented during feature-permission testing;
it does not add a registration ACK protocol or prove coordinator acceptance.

Node status/diagnostics and `eefn setup` also expose `input_channel`:

| State | Observation |
| --- | --- |
| `unavailable` | No local sender exists, or its session receiver has closed. |
| `ready` | The observed local sender is open and has bounded queue capacity. |
| `busy` | The observed open sender has no remaining immediate queue capacity. |

The observation reads only the existing mailbox mutex and channel counters. It
never sends, drains, reserves capacity, probes a coordinator/model, issues a grant,
creates another queue, changes settings or retries input. Capacity and closure can
change immediately after observation. Ready is **not** a lease, message delivery,
healthy coordinator, successful inference, permission to execute, final result or
promise of future admission. Receipt-support configuration is a separate fact.

The client retains the receiver in its exact connection future. When that receiver
drops, current status reports unavailable even if a historical connected label has
not yet been refreshed. Pending/sent waiters preserve their existing uncertain
outcomes. Reconnecting installs an empty new queue; old submissions/results are
not replayed or migrated. An empty closed queue is not reported as ready.

## Setup and compatibility

`eefn setup` explains the observed queue state separately from transport, saved/
applied interpreter and receipt support. Connected-but-unavailable/busy observations
derive fixed wait/inspection steps; these steps do not submit input or replay an
uncertain request. Older setup reports without the additive field (or explicit
null) remain accepted as unknown, never inferred ready from the connected label.
Malformed state claims are rejected by the existing strict setup-report validator.
Status and diagnostics add the field without changing their old fields or wire
registration/heartbeat contracts. ESP firmware is not given a new channel protocol.

The earlier `permission-commands-Q88ctK` failure saw a TLS transport label while
connection startup was incomplete and returned `concrete_not_sent`, with zero
durable jobs. That retained failure does not prove the source-order window caused
it. The permission fixture was separately corrected to wait for connected state;
this increment additionally publishes Connected after channel installation and
exposes direct mailbox observation. It does not erase the old failed evidence or
retry its request.

## Verification (2026-10-14)

- Fresh default and dashboard-disabled workspace/all-target suites each passed
  **677 tests**, with one ignored. Eleven focused input tests also pass. New
  cases cover publication after receiver installation, absent/open/full/closed
  queue observations without draining, empty reconnect without old input replay,
  setup/status/diagnostic agreement, busy steps and legacy unknown observations.
- Fresh binaries/examples, JavaScript syntax, backend/browser source-boundary,
  diff whitespace and formatting checks pass. A test-only assertion layout was
  formatted after the full suite; no logic changed.
- `setup-check-S3VBqo` passes actual headless plain-text/JSON commands, including
  ready connected input, unavailable paused input, explicit reconnection and
  older reports with no field staying unknown. No inference/download is requested.
- `permission-commands-vpYP6q` passes public local/approved coordinator permission
  controls and real reviewed file work. A separately approved native pipeline
  stopped on permission loss before its later write; held creation-verified
  root/child handles observed exit after **194 ms**, before the 10,000 ms deadline.
  This remains scoped containment evidence, not general external-effect Stop.
- `node-output-JFER9J` runs an actual API-only node against an encrypted legacy
  loopback peer, retires blocked framed output and reconnects after **6,118 ms**.
  Runtime ID/config remain unchanged, the replacement queue reports ready, and
  existing no-response/work-replay assertions pass. It is not pinned-TLS,
  native-termination or physical-network evidence. Fixture processes exited.

The earlier failed permission startup fixture remains retained, with no replay
or gate relaxation. These local passes do not establish full physical/load
acceptance or stable v0.4.0 completion.

## Remaining scope

Coordinator registration/acceptance acknowledgement, atomic cross-layer readiness,
full disconnect/load/physical two-PC acceptance, automatic first-run bootstrap and
the remaining stable release roadmap stay open. No installed node, published
installer, model default, update selector or UI asset is changed.
