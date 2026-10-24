# Execution and terminal reply ownership (unreleased)

A peer can receive a complete encrypted response before the sender's `flush`
future finishes. Keeping the execution permit until that write returns can make
the peer's next sequential command falsely busy, especially at work capacity one.
A controlled duplex writer reproduces this: the peer decodes the whole response,
flush remains gated, and the next command is refused. The original regression
fails at its unchanged assertion in `.validation/admission-reply-baseline.log`.
This establishes the mechanism, not retrospective proof of every prior capacity
refusal or a physical-network timing guarantee.

## Ownership and bounds

Each admission reserves execution capacity and a terminal-reply slot before any
work starts. There is no unbounded admission queue or automatic retry. The saved
`request_limits.work` and `request_limits.control` values, ranges and defaults
are unchanged; they bound executing network futures, not backend side effects,
CPU/RAM or physical termination.

At terminal completion, execution ownership is released before response encoding
or transport waiting. Reply ownership stays through encoding, the existing writer
queue, flush, error and cancellation. A pending reply does not retain execution
permission, and reply ownership cannot authorize another operation. Successful
reply reservations are released inside the writer lock before its next handoff;
failed/unconfirmed writes retire framing before remaining ownership is dropped.

Work and reserved controls have separate reply pools. Each pool permits at most
its execution limit plus one admitted pending/executing reply. The additional
single slot per class accounts for a complete reply still flushing when its peer
submits the next command. It does not increase execution concurrency or borrow
reserved control slots. Even if all executions finish, pending reply futures are
bounded by `work + 1` and `control + 1`; exhausted reply capacity refuses a new
request before execution. This is an explicit bound, not an unlimited output
queue, byte quota, hard real-time or memory-usage guarantee.

Ordinary controls, reviewed grants and peer grants retain execution ownership
through their actual execution/lease/acknowledgement phases and change ownership
only for their terminal response. Blocking stop/receipt readers keep execution
ownership inside their blocking callback even if its network waiter is cancelled.
The async waiter owns the reserved reply slot, including a callback-panic reply;
callback completion returns execution before transport output.

The existing encrypted framing, five-second write/queue deadline, encoded-size
bound and retirement-on-failed/partial/unconfirmed-write behavior remain. Delivery
is still distinct from acceptance, and an output failure is not rollback or proof
that earlier effects stopped. No rejected/uncertain operation is replayed.

## Verification boundary

The original stop checkpoint retains its metadata failures and twenty-four
passing native suites in [setup stop evidence](MODEL-SETUP-STOP.md). They are not
accepted as a complete combined run. The deterministic regression above fails
before this ownership change. The first focused node-library run then passes all
731 tests, including unchanged frame-retirement tests, execution/reply bounds,
independent reserved controls and blocking-callback cancellation. That run
predates final error-message wording and additional native diagnostics/stress;
fresh full acceptance remains required, including the later callback-panic test
and inside-writer-lock reply handoff.

The metadata fixture keeps its one-slot work limit and existing assertions. It
adds sixteen distinct acknowledged no-op commands without retry, verifies the
settings file is unchanged, and privately retains failure/child logs rather than
losing error context. These are test observability/coverage changes, not relaxed
admission, timing or authorization. The first post-change metadata fixture
(`model-metadata-5wzI6Z`) passes all seventeen acknowledged no-op replies (the
original plus the sixteen-command burst), then times out at the older stop
expectation. Its private logs and read-only job/receipt inspection show retained
`grant_stop_requested_effects_unknown` and interrupted tracking: the expectation
of cancelled tracking/completed backend output contradicts the existing exact
stop contract and unit tests. It is not a passing fixture.

The corrected fixture keeps the backend response held until interruption and
the exact unknown stop result are observed. It checks read-only retained stop
scope, false physical-stop claims, no late output/replay and explicit abandonment
before removing uncertain tracking. The separate completion/pause-refusal case
remains unchanged. It does not accept either outcome opportunistically. Fresh
fixture `model-metadata-awzbYe` passes those strict expectations and the no-op
burst before the final writer-handoff adjustment; that standalone pass does not
replace fresh combined validation. All earlier failures remain retained.

Fresh binaries must pass
that fixture and existing blocked-output/reconnect and reviewed-input/work tests,
as well as both full Rust feature modes and broader native regressions.

No installed/remote node, user permission, model/default, release version,
installer, Defender policy, update feed or MSI tooling changes. No full first-run,
approved interpreter quality, physical two-PC or stable-release claim is made.

## Combined local acceptance (verified 2026-10-24)

The final frozen source passes both locked/offline workspace/all-target feature
modes: 1,073 tests plus one existing ignored profiler each, across fourteen target
groups. Fresh headless apps/helpers build. One complete sequential run passes all
twenty-eight native suites, including model metadata (`model-metadata-ElvZ3O`),
blocked output (`node-output-vrVrh5`), reviewed work (`request-review-KU6Nui`),
input workflows (`request-input-hxDekE`) and setup stop (`model-setup-39hW9r`).
The metadata result includes all sixteen strict acknowledged no-op commands,
unchanged settings and deterministic exact-stop/unknown evidence checks.

The read-only audit `.validation/verify-reply-ownership.mjs` verifies all public
results, configured suite order, both complete Rust totals, 295 source/resource/
workflow hashes and nine native artifact hashes. Every hash matches; no matching
fixture processes remain observed. Evidence is `.validation/reply-ownership-acceptance.json`,
the `reply-ownership-{headless,default}.log` files and `reply-ownership-native.log`.
The previous failed and partial runs remain separate evidence. This is not a
claim that every historical capacity refusal had the same cause.

The workflow now configures twenty-six hosted native suites, retaining the two
local-only stock-runtime suites separately. Workflow lint passes; a fresh hosted
run for the source checkpoint is still required. Local checkpoint acceptance is
not publication of new installers, a real default/model-quality decision,
physical multi-node acceptance, successful signing/Defender clearance or stable
v0.4.0 acceptance.
