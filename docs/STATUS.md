# Architecture conformance status

Newest checkpoints appear first. Earlier entries retain the implementation and
verification boundaries at their own checkpoint, not claims about later code.

## Unreleased: opt-in finite microphone-to-speaker peer execution

Native app startup can now opt into finite PCM16 capture/transfer/playback using
existing Python adapters, paired reviewed jobs, ordinary consumed grant journals
and native authenticated peer sockets. Defaults remain disabled. Advanced
resource/receiver configuration is still required; no installed node, release,
model, update policy or dashboard source changed. See
[content, authority, configuration and remaining scope](PEER-TRANSPORT.md#finite-capture-to-playback-unreleased-advanced-configuration).

Schema-version 2 separates a format-derived maximum from the actual captured
digest. Capture is canonicalized to strip WAV metadata; the receiver validates
the entire bounded WAV before playback and withholds final ACK until the sink
returns. Both endpoint reports must match content. Capture/playback do not occur
on review/save or approval alone, and reports retain no raw audio or physical
sound confirmation. Legacy static-buffer schema-version 1 serialization remains
compatible. Transport and nested media providers share existing bundle admission,
with one executor future per endpoint rather than double-counting each child.

The first public run exposed an omitted explicit-template allowance in durable
review. Retained review now admits this explicit template without adding it to
LM methods; capture and playback must also appear in reviewed capabilities.
Known media plugins cannot bypass disabled feature switches. Saved permissions,
resources and receive-route changes are rechecked under the existing consumed
parent watcher. Changed routes, malformed capture and uncertainty do not replay.

Fresh headless binaries/examples build. Public `local-pipelines-KEHnYZ` and the
stricter `local-pipelines-5Knn5J` pass actual command, pairing, approval, journal,
native TLS and real cached CPython paths with synthetic microphone/speaker
adapters only. They check matching reports, no effects on save/approval,
permission/route revocation, malformed capture, dependency accounting, replay
refusal and healthy fresh work after failure. No real device is opened.
Standalone `python-bridge-s8o1Ys` passes bounds, Unicode, deadlines, descendants
and cancellation; its cancellation fixture now waits for observed plugin/child
startup instead of a fixed delay that could expire before invocation.
`model-downloads-J5RlLs`, `node-credentials-0IYn5S`, `request-drafts-izS5iA`,
browser copy/status smoke and the recursive backend/browser source guard pass.

Both locked/offline workspace/all-target modes pass **831 tests plus one ignored
each**, including no-dashboard: EEF 260 plus one ignored profiler, owner CLI 17,
installer 6, EEFN 528 and node CLI 20. Final headless binaries/examples build;
fixture apps/helpers are terminal. Versioned releases and update feeds remain
unchanged.
Physical two-PC/current installer acceptance, stronger observed stop/resource
quotas, non-coder relay setup, default model quality/bootstrap, continuous media,
virtual call devices/mixing, presence, mesh/ESP/Bluetooth, measured migration,
coordinator clustering and signing/Defender release acceptance remain unfinished.

## Unreleased: bounded Python capture/extension boundary

The existing Python bridge now bounds request encoding, stdout and stderr,
drains both output pipes while writing input, and places the full exchange under
one original deadline. Parent grants and local pipeline stages intersect the
local response ceiling with their existing result/retained-data budget.
Metadata preflight is awaited under that deadline; compact UTF-8 exchange avoids
locale-dependent decoding and ASCII-escape expansion of non-English data.
Windows uses the existing owned job before the host resumes; root exit retires
descendants within the same deadline. Timeout/error/cancellation retains existing
uncertainty, never permits replay or proves physical device release.

Optional validated `python_bridge` settings use the shared configuration service
and existing remote approval, with saved/applied/enabled/restart status. Decoded
out-of-range bridge limits disable Python startup without disabling core transport.
Raw plugin output/error/trace is no longer forwarded in errors; static
`python_*` report causes preserve useful failure diagnostics without capture data.
See [limits, lifetime and remaining scope](PYTHON-BRIDGE.md).

Both final locked/offline all-target modes pass **823 tests plus one ignored
each**, including no-dashboard. Fresh headless binaries/examples build. Actual
cached CPython 3.14.5 passes standalone `python-bridge-nsRyTw` bounds, privacy,
Unicode, deadline/cancellation and descendant/inherited-pipe cases. Public
`local-pipelines-tq2Urg` additionally runs synthetic capture through real
reviewed/approved/consumed single grants and pipelines: static causes, unknown
outcomes, no replay and healthy fresh work after failure. It uses no real camera.

Final-binary `model-downloads-fCfgrN`, `node-credentials-sv2zpt` and
`request-drafts-a26Vua` pass existing public commands, history and restart/no-replay
regressions. Browser copy/status smoke and recursive backend/browser guard pass.
All matching fixture apps/helpers are terminal. No installed/remote node, model,
update policy, published installer or release changed.

This corrects a prerequisite in the actual capture path; it does not enable
installed-node peer media, provide continuous capture, impose Python heap/CPU
quotas, complete physical two-PC acceptance or close the stable-release gates.

## Unreleased: native finite peer delivery lifecycle, not installed-node media

Opt-in `NodeClient` delivery now consumes the existing grant, uses observed
control/node/coordinator identities, and owns one scoped TCP route under the
existing lease and permission watcher. Destination binds before readiness ACK;
bind failure is consumed uncertainty without releasing source dispatch. Ordinary
grant completion still requires its matching saved ACK.

Explicit embedding-owner sources share bounded immutable memory; receive
resources verify and discard. Neither payloads nor private keys enter status,
registration or execution receipts, and unbound resources remain unavailable.
Native app startup still supplies no producers or peer permission/admission:
installed-node/public peer commands and media remain unavailable. See
[contract and boundaries](PEER-TRANSPORT.md#native-finite-delivery-executor-unreleased-app-startup-remains-disabled).

Real native client regressions exercise coordinator paired dispatch, consumed
journals, loopback TCP/mutual TLS, exact joint outcomes, replay refusal, binding
failure, changed-source refusal and control-disconnect cleanup. The disconnect
case reads retained node state read-only without reopening/reconciling its
journal. This is one-PC synthetic finite transport, not physical two-PC, actual
capture/playback, strong remote stop or release acceptance.

Both final locked/offline all-target modes pass **815 tests plus one ignored
each**, including no-dashboard. Fresh headless binaries/examples build. Public
`local-pipelines-KQuE4y`, `model-downloads-KGVT1P` and
`node-credentials-rwuVyQ` and `request-drafts-Z8puop` pass existing command, history, restart/no-replay and
credential/pairing regressions. Browser initialization/copy/status smoke and
the recursive backend/browser guard pass. These public app regressions do not
activate native peer producers, which remain unavailable in app startup.

No installed/remote node, model, update policy or release changed. Producer/sink
integration, discovery/disclosure, aggregate bandwidth supervision, joint
recovery, strong stop and physical/release gates remain open.

## Unreleased: atomic paired coordinator issuance and joint outcomes

The explicit finite pair template retains origin review, fixed endpoint bindings
and owner approval in the existing journal. Both exact grants and sealed delivery
are checkpointed atomically before dispatch. A durably saved destination ACK
gates source dispatch; both scope-bound results are required for parent success.
Ordinary reviewed jobs remain serial. Failed writes cannot retain half a pair;
restart preserves issued authority as uncertain without replay. One-grant paired
recovery/abandonment and reusable paired automation authorization are refused.
See [contract and remaining boundaries](PEER-TRANSPORT.md#coordinator-paired-jobs-unreleased-native-delivery-still-unavailable).

Seven focused tests pass, including the real coordinator with authenticated
control sockets and emulated endpoint receipts. This is not native peer-data or
physical-node acceptance. Native startup still advertises no peer support and
implements no delivery/socket handler: no user-facing native peer session is
enabled. No installed/remote node, model, update policy or release changed.

Both final locked/offline all-target modes pass **808 tests plus one ignored
each**, including no-dashboard. Fresh headless binaries/examples build. Public
`local-pipelines-BVbdaV`, `model-downloads-OAapCW`, `node-credentials-KuhF7n`
and `request-drafts-zsRVWd` pass existing command, history and restart/no-replay
regressions; all fixture apps/helpers are terminal. Browser copying/status smoke
and the recursive backend/browser guard pass. These public regressions do not
exercise a native peer session, which remains unavailable.

## Unreleased: consumed finite peer admission, still no app peer sessions

The experimental library now binds each signed delivery to a full exact grant
digest and admits finite transport through existing `GrantLease`,
`AcceptedGrant` and `GrantLedger`, not a second authority or journal. It shares
the existing deadline/policy watcher, drop-to-unknown behavior and retained
reports. Local permission/resource/session-capacity and journal checks precede
admission; ordinary grant execution refuses the special delivery contract.
See [contract and limitations](PEER-TRANSPORT.md#consumed-finite-transport-admission-unreleased-follow-up).

Fresh native regressions exposed a real Windows parser stack overflow, not a
Defender removal. LLDB traced Clap command-tree construction inside Tokio entry;
argument parsing now precedes the async runtime while preserving version handoff
before parsing. The executable's 1-MiB stack reservation is unchanged; a 1-MiB
parser regression and bounded-future checks pass.

Both final locked/offline all-target modes pass **801 tests plus one ignored
each**, including no-dashboard, ten new peer-admission cases and two CLI gates.
Fresh headless binaries/examples build. Final-binary public
`local-pipelines-c8jcbY`, `model-downloads-1dj8ED` and `node-credentials-m62kSj`
pass existing command/restart/no-replay and private credential/pairing regressions.
They are terminal with no matching fixture processes. The browser harness now
waits for visible hash navigation before inspecting edited configuration; no
application UI changed or assertion was removed. Two browser runs and the
recursive backend/browser guard pass. Initial failures remain recorded, not
substituted for final-artifact verification.

App startup does not enable it and no capability/command/listener is exposed.
Static byte manifests and transport-only receipts are not camera/file/audio
production or sink delivery. Paired coordinator planning/atomic issuance,
discovery/disclosure, actual socket admission, supervision/strong stop,
aggregate quotas, producers/sinks and physical acceptance remain open. Existing
reviewed jobs remain serial. No installed/remote node, model or release changed.

## Unreleased: peer transport library foundation, not node session support

The experimental `eefn::peer` library prepares recipient-encrypted,
coordinator-signed bundles without exposing raw private material in inspectable
parameters. Scoped mutual TLS and existing encrypted newline framing enforce
exact peer/direction/sequence, bounded frames/windows/application bytes/rate and
original expiry. Protocol/time/IO failure and cancelled partial IO fence the
channel; acknowledgements must match an exact sent prefix. See
[contract, limits and outstanding integration](PEER-TRANSPORT.md).

Both final locked/offline all-target modes pass **789 tests plus one ignored
each**, including no-dashboard and 21 new peer tests. The backend/browser source
guard now includes nested runtime modules and passes. Local synthetic TLS/TCP
evidence is not public peer command, physical two-PC or media acceptance.
Fresh headless binaries/examples build; final-binary public
`local-pipelines-TX38Bw` and `model-downloads-MB5qua` pass existing command,
history, permission-loss and restart/no-replay regressions. Both fixtures are
terminal with no matching EEF/EEFN/native-helper processes. Browser smoke passes
initialization, copy stability and truthful installation status, without UI edits.

No app listener, command, permission, capability, grant operation or media path
enables this module. Existing approved jobs and consumed grants must remain
the authority. In particular, stateless bundle opening does not prevent session
replay across reconnect/restart. Durable paired admission, current permissions,
resources/disclosure, stop/revocation, aggregate quotas, public command integration
and physical two-PC/media acceptance remain open. No model, installed/remote
node, update policy or release changed.

## Unreleased: durable model-install admission and outcomes

The existing node installation service now commits bounded receipts before
transfer admission and before publishing terminal outcome. Full app replacement
preserves an unfinished admission as interrupted, with no restored percentage,
selection change or download replay. Earlier IDs remain inspectable and cannot
be readmitted; capacity refuses new work instead of evicting history. Ordinary
runtime restart still keeps the same live transfer. See
[storage, configuration, recovery and limits](MODEL-INSTALL-RECEIPTS.md).

Public headless `model-downloads-QpAF1W` passes full process/runtime replacement,
terminal history, exact cancellation, lost replies, non-replay and read-only
inspection. Receipt-policy failure leaves core controls usable; explicit
revalidation after correction still cannot replay an old ID. Browser regression
passes copy stability and truthful unknown/interrupted/progress rendering. This
is a minimal status fix, not resumed UI redesign. Fixtures use synthetic data and
mock provider streams, not real model downloads or physical two-PC acceptance.

Both final locked/offline all-target modes pass **768 tests plus one ignored
each**, including no-dashboard. Fresh headless binaries/examples build;
final-binary `model-downloads-AtDkU5` repeats the full public chain successfully.
The additional service fault case observes an intact verified cache/index with
failed final receipt persistence: live unknown, then interrupted on a new owner,
never inferred success or file deletion.
Final-binary `local-pipelines-LmJsgu` also passes finite/native/recurrence and
durable-progress/restart regressions; both fixture chains are terminal with no
matching fixture processes left.

Automatic bootstrap/default quality, partial-file reclamation/resume, physical
and full release gates remain open. No installed/remote node, model default,
release/feed or shared-service settings changed.

## Unreleased: claim-local scheduler validation and precise fence failures

Due claims reuse dependencies from their fully validated read phase, discard
that view before staging, and build a new complete view afterward. Final-fence
errors distinguish lateness, backward time, changed authority and insufficient
remaining execution time. The existing rollback/no-catch-up rules and every
authorization/window/quota remain unchanged. See
[contract and evidence](AUTOMATION-SCHEDULES.md#claim-local-validation-frames-2026-10-18).

Both final all-target modes pass **758 tests plus one ignored each**, including
no-dashboard; fresh headless binaries/examples build. Public scheduling
`mc895D` passes with eight committed claims (local median 116 ms, maximum 197 ms;
pre-change measured median 153 ms, maximum 244 ms). This is a single debug-build
comparison, not loaded/physical timing acceptance. Stale pipeline/status text
calling ordinary one-second acceptance failed has been corrected; earlier
successful one-second evidence already existed and old failures remain recorded.
Final-binary public `local-pipelines-6ldaE8` also passes finite/native/recurrence,
durable-progress and restart/no-replay regressions. Both harnesses are terminal
with no matching fixture processes left. No installed/remote node, model, UI or
release/feed changed.
Full bootstrap/model-quality/media/peer/physical/release acceptance remains open.

## Unreleased: durable finite-pipeline progress through existing receipts

The executor persists one bounded latest observation per finite pipeline grant.
Initial progress shares the consumption transaction; stage admission checkpoints
precede effects, and failed writes prevent the next stage. Existing local,
origin-node and coordinator receipt commands expose it without UI dependencies,
payload retention, reconciliation, replay or resume authority. Parent terminal
evidence remains separate; completed stages do not manufacture parent success
or proof of stopped effects. Legacy missing progress stays absent. See
[contract and verification](LOCAL-PIPELINES.md#latest-durable-finite-pipeline-observation-unreleased).

Both final locked/offline workspace/all-target suites pass **755 tests plus one
ignored each**, including no-dashboard. Fresh headless binaries/examples build.
Public `local-pipelines-dOSfu4` passes completed, failed and pending observations,
all three existing receipt routes, read-only inspection, scope denial, permission
loss and executor/coordinator restart without replay. This is two authenticated
logical nodes on one PC with a synthetic model, not physical two-PC acceptance.
Broader `request-drafts-3EKRho` also passes authenticated receipt inspection,
uncertain restart, tracking abandonment and no-replay regressions. Both harnesses
completed; no fixture EEF/EEFN/native-helper processes remain.

An intermediate suite caught a small-journal regression. Progress tables and
reservations are now pipeline-only and first creation shares the claim
transaction; the original 64-KiB ordinary-work test passes unchanged. Continuous
workloads, strong stop, physical resource limits, loaded/physical timer
reliability, default interpreter/bootstrap and physical/release gates remain open.
Ordinary one-second scheduling has separate passing public evidence; see
[the scheduler follow-up](AUTOMATION-SCHEDULES.md#request-local-history-readers-2026-10-13).
No installed/remote node, UI, release/feed or model changes.

## Unreleased: controlled restart waits for owned model cleanup

Node restart now requests shutdown and awaits the existing runtime instead of
dropping it. Cancelled cleanup retains original process/tree handles. One applied
termination budget covers admission, concurrent group observation and root
reaping; an old-server barrier prevents replacement initialization or version
handoff while cleanup remains uncertain. A later explicit owner restart inspects
that same barrier, with no inference/workload replay or automatic restart loop.
Read-only exit observation can release ordinary failed-load objects only after
the root and Windows job are confirmed exited; live retirees block new loads.
See [contract, limits and verification](MODEL-SHUTDOWN.md).

Both final all-target modes pass **745 tests plus one ignored each**. Public
startup `r7pv8V`, lifecycle `Jz7wAo`, policy `zONquI`, and cached real CPU model
`HV3XFX` pass. Native observers verify loading/ready restart descendant exits;
the real run covers controlled restart, unload/reload and idle/on-demand use.
No model download/copy or model-quality approval follows; final processes are
terminal after cleanup. An intermediate failed withdrawal fixture is retained
and documented, not recategorized as a pass.

This is not observed abrupt process death, physical RAM/VRAM release, shared
Ollama termination, continuous pipeline stop, default model/bootstrap or complete
physical/release acceptance. No installed-node/UI/release changes.

## Unreleased: observed targeted model termination

Explicit and idle owned-GGUF unload retain their generation/control fence while
`unloading`; they publish `unloaded` only after termination observation. Windows
observes the exact owned job empty and reaps its root under one existing deadline;
other platforms retain root-only guarantees. Error, timeout, cancellation, missing
ownership or a late ready result becomes `unload_unconfirmed`, not success. That
state blocks inference/load/startup in the same runtime and survives group cleanup.
Late results cannot overwrite a closed/replaced generation. Pending accounting is
unavailable, not fabricated zero usage; legacy registration remains busy/error
and unavailable. See [contract and verification](MODEL-LIFECYCLE.md#targeted-unload-observations-unreleased).

Both final locked/offline all-target suites pass **739 tests plus one ignored
each**, including no-dashboard; fresh binaries/examples build. Public lifecycle
`bMBUGH` independently observes nested owned-process exits and preserves the
sibling. Startup `L3izuZ`, lifecycle policy `9E2QBk`, and cached real CPU model
`zSuIyJ` pass, including explicit/idle/on-demand unload/reload without downloads
or model-quality approval. All validation processes are terminal after cleanup.

That targeted checkpoint did not observe ordinary group shutdown; the subsequent
controlled-restart increment above extends it. Neither proves physical RAM/VRAM
release, reversibility of external effects, shared Ollama termination, continuous
pipeline stop or complete bootstrap/physical/release acceptance. No force unload
or installed-node/UI change is added.

## Rejected balanced-illustration comparison (2026-10-18)

A replacement for the ambiguous-only prompt illustration was evaluated on both
frozen suites and both existing backends, without downloads or installed-node
changes. All eighty proposals were structurally accepted, but matches fell from
51/80 to 36/80 and eighteen required questions were lost. Removing all unnecessary
questions and literal sample-goal copies did not establish reliable grounding.
The candidate and its isolated unit test were removed; production source is
restored exactly to `c0151b3`. See [criteria, complete results and restoration
checks](INTERPRETER-QUALITY.md#rejected-balanced-illustrations-2026-10-18).
This is negative quality evidence, not a shipped prompt improvement or default
approval. Further method/model/grounding work, bootstrap and physical/release
acceptance remain required; prompt additions alone are not an acceptance strategy.

## Two-backend interpreter quality baseline (2026-10-18; not approved)

The production-preview evaluator now supports existing shared Ollama as well as
verified owned CPU GGUF. It separates provider reports from verified file/runtime
identity and owned CPU samples, explicitly prepares the selected shared model,
and stops further cases after uncertain outcomes or changed tag/residency/context.
Reference tools support both typed profiles and legacy CPU evidence without
confusing ownership or silently rewriting existing records.

Both frozen suites complete on both backends: all eighty proposals are accepted
structurally. CPU matches remain **14/20 and 12/20**, with all forty parsed
proposals identical to the prior baseline. Existing Gemma/Ollama matches
**13/20 and 12/20**; it asks unnecessary questions on all fifteen clear cases.
Neither model is default-approved. See [exact runs, identity scope and remaining
quality defects](INTERPRETER-QUALITY.md#two-backend-frozen-case-baseline-2026-10-18-no-default-approval).

Synthetic shared-protocol, bounded metadata, proxy/redirect, no-replay/abort,
argument, frozen scorer, reference and source-boundary checks pass. This is a
test-tool change: production Rust/prompt/schema/scorer stay unchanged from the
735-test checkpoint. No models downloaded/copied, installed/physical nodes,
UI, shared settings, release or stable feed changed. The next interpretation
increment must address unnecessary clarification without losing required
questions. Default bootstrap and physical/release gates remain unfinished.

## Unreleased: reasoning-aware completion failures (2026-10-18)

Incomplete interpreter responses keep the existing generic failure code and add
an optional static completion category. Reasoning-only output exhaustion,
ordinary output limits, tool calls, filtering, explicit unfinished generation and
unconfirmed finish metadata now have text-free explanations. The shared engine
and owner services never promote reasoning/partial output, call tools, retry,
increase caps, select a replacement model or grant authority. Completed output
keeps its old normalized shape; logic contains no model-name exceptions.

Both locked/offline workspace/all-target suites pass **735 tests plus one ignored
each**, including no-dashboard; fresh binaries/examples build. Public preview and
pinned-TLS input/answer regressions verify classifications, redaction, partial
history and no replay. The opt-in bounded test observer preserves bytes and keeps
raw responses private, with synthetic admission/privacy/bounds tests passing.
See [contract and measured scope](INTERPRETER-COMPLETION.md).

Captured Qwen evidence confirms `think:false` nevertheless yielded zero answer
bytes and reasoning-only `length` termination at 512 tokens. A separate existing
Gemma model completes the same bounded synthetic preview in **7,196 ms** after
explicit preparation, but asks an unnecessary format question. This proves one
completed structurally accepted response, not semantic/default model approval.
No downloads, installed/physical node, UI, service-setting, release or stable
update-feed changes. Quality comparison, bootstrap and release gates remain open.

## Unreleased: explicit owner inference wait (2026-10-17)

Seven model-backed request commands accept an explicit 1–120-second per-call
allowance. Default inference remains 20 seconds; legacy request shapes, backend
headroom, token caps and default workflow/client waits are unchanged on omission.
Input/answer workflows propagate explicit choices to both model stages and
extend their overall waits accordingly. Timing is outside model output, origin
context and execution authority, not a saved preference or automatic retry.

Both locked/offline workspace/all-target suites pass **732 tests plus one ignored
each**, including no-dashboard; fresh binaries/examples build. Public preview
accepts a 31-second mock reply with an explicit allowance and checks short/default
timeouts. Public input/answer tests cover all stages, retained partial history,
and no inference replay when an existing receipt gets a different allowance.
Bounds, pre-write rejection, compatibility and source checks pass. See
[contract, exact fixtures and remaining scope](INFERENCE-TIMING.md#verification-2026-10-17).

One separate real existing-Ollama preview with explicit 120 seconds returns after
**39,689 ms**, but fails `interpretation_incomplete`. Preparation succeeded;
provider logs record 512 generated tokens, matching the unchanged cap. No proposal
or semantic quality is approved, no token/schema/validator bound is loosened, and
the previous 20-second failures remain intact. No downloads, installed/physical
node, UI, release or stable-feed changes. Reliable interpretation/default model,
automatic bootstrap and physical/release acceptance remain unfinished.

## Unreleased: explicit Ollama model preparation (2026-10-17)

The existing model lifecycle worker/queue now accepts owner-directed
`prepare-ollama`, locally and over approved authenticated coordinator commands.
It checks the applied selection/runtime/policy and exact installed digest, uses
empty-message loading, and confirms identity/residency afterward. Saved `auto`
works only when Ollama is actually applied. Reports distinguish shared effects
from owned GGUF state; cancellation/timeout never forces an unload or claims
server work ended. Receipts are bounded, runtime-local and never replayed.

Both final locked/offline workspace/all-target suites pass **726 tests plus one
ignored each**, including no-dashboard. Fresh binaries/examples build. Public
synthetic preparation, observer and existing preview/policy regressions cover
identity/authority/cancellation and preservation of owned lifecycle behavior.
The real existing-Ollama run completes preparation in **12,982 ms**, matching
reported digest and 4,096 context. Its subsequent warm preview **still times out**
after **20,026 ms**: no accepted proposal or quality approval, no automatic retry
or changed inference deadline. See [exact evidence and limits](OLLAMA-PREPARATION.md#verification-2026-10-17).

This removes a missing explicit loading operation, not full automatic bootstrap,
model-quality/latency acceptance, physical two-PC or release gates. No downloads,
model copies, shared-service changes, installed app/physical node updates,
dashboard, release version or stable update feed changed.

## Unreleased: owner-controlled Ollama context (2026-10-17)

Local/offline and owner-approved coordinator commands can save/reset the optional
Ollama context size through the existing policy service. Values are bounded,
saved/applied state remains separate until explicit restart, and callers cannot
override the applied engine option. Inherited policy/wire digests stay compatible;
no shared-service setting, model definition, permission or default changes.

Both locked/offline workspace/all-target suites pass **720 tests plus one ignored
each**, including no-dashboard; fresh binaries/examples build. Mock-provider
preview and policy/lifecycle regressions pass, along with observer admission and
synthetic success/rejection checks. The one real existing-Ollama cold-load preview
fails at the unchanged 20-second deadline: local provider logs confirm 4,096
context but loading is canceled before a proposal is returned. No retry or
automatic timeout increase hides this failure. See [contract, evidence and
limits](OLLAMA-CONTEXT.md#measured-checkpoint-2026-10-17).

Explicit shared-provider preparation/readiness, real-model quality/default
approval, full bootstrap and physical/release gates remain unfinished. No model
download/copy, installed/physical node, UI, release or stable update feed changed.

## Optional Qwen3 1.7B evaluation (2026-10-17; no default approval)

Added one pinned, manual-only Q4_K_M catalog option with publisher-declared
Apache-2.0 provenance and exact artifact size/digest. The first catalog entry,
owner replacement/customization, provider/role choices and sampler defaults stay
unchanged. The mutable Ollama alias is not the tested GGUF's artifact identity.

`real-model-GJWNnk` passes explicit install-plan/install/status, full size/SHA
verification, pinned-TLS reviewed CPU inference, stable restart identity,
retained results without replay, targeted unload/reload, idle and on-demand
residency, and model withdrawal. This downloads one 1,107,409,472-byte artifact;
the four subsequent quality runs reuse it without copying or downloading again.
It is retained only in ignored validation storage; about 3.1 GB remains free.

All eighty quality proposals are structurally accepted, but the temperature-zero
original/generalization runs match 9/20 and 7/20, and the declared alternate
sampling profile matches 8/20 and 6/20. Literal copied goals occur in 18, 17, 17
and 17 cases. No required question is lost, but consequential/ongoing requests
are repeatedly misclassified and clear questions remain unnecessarily unresolved.
Neither profile is default-approved; the fixed 2B reference still performs better
on this development task, without itself being approved. There is no prompt,
schema, rubric or validation relaxation. See
[source and lifecycle](MODEL-CANDIDATE-EVALUATION.md#optional-qwen3-17b-candidate-2026-10-17-not-default-approved)
and [all four quality runs](INTERPRETER-QUALITY.md#qwen3-17b-candidate-2026-10-17-no-default-approval).

Both locked/offline workspace/all-target suites pass **715 tests plus one ignored
each**, including no-dashboard. Fresh binaries/examples build; node/helper hashes
match the measured quality artifacts. All four metadata-only schema/reference
captures pass, as do formatter, diff, scorer, argument and backend/browser source
checks. Fresh `model-metadata-KU5Yaj` passes the mock-provider public inventory,
selection and approved remote protocol regressions. These are isolated one-PC
tests, not physical two-PC, resource-quota or
general coding/chat acceptance. No installed/physical node, UI, release artifact
or stable-only update feed changed. Default interpreter/bootstrap and remaining
release/roadmap gates remain unfinished.

## Unreleased: generation clarification consistency (2026-10-17)

The generation schema mirrors the existing independent validator's cross-field
rules with two complete alternatives. Clear proposals require null question,
false flag and known intent/workload; questioning proposals require nonempty
question/true flag and permit uncertainty. All ten fields, capability/text/array
limits, prompt, byte/authority checks and immutable review state remain unchanged.
Providers may ignore/reject constraints; no repair, unconstrained retry or
fallback is introduced. Method generation retains its separate contract.

Both locked/offline workspace/all-target suites pass **715 tests plus one ignored
each**, including no-dashboard. Tests cover all 48 clarification combinations,
the maximum capability vocabulary and unchanged independent rejection.
Fresh binaries/examples build. `request-preview-Ece8DH` checks both-schema
forwarding and a mock provider's inconsistent output; `request-input-wIkILV`
passes pinned-TLS input/answer/source-lineage and schema-rejection regressions.
These are isolated same-PC fake-inference fixtures, not real-Ollama acceptance.

Real CPU suites `interpreter-quality-Bvx75l` / `interpreter-quality-iDUGub`
accept 20/20 each and match 14/20 and 12/20. All forty parsed proposals equal the
historical context baseline; copied goals/semantic failures remain. Default
quality is not improved or approved. New private generation references include
schema/message fingerprints; exact capture, changed/malformed hash refusal,
legacy report shapes and exclusive output pass metadata-only tests. Fresh
node/helper hashes match the evaluated binaries. See
[scope, hashes and evidence](INTERPRETER-QUALITY.md#generation-clarification-consistency-2026-10-17-unreleased).

Format, JS syntax, source dependency boundary, frozen scorer, evaluation-argument
and diff checks pass. No model download/copy, installed/physical-node update,
release, UI or stable-only update feed changed. Default model/bootstrap,
complete first-run, physical and other release gates remain unfinished.

## Rejected interpreter checklist experiment (2026-10-17)

The real CPU interpreter experiment retained the prior prompt and appended
request-specific goal, intent/workload and clarification guidance. Both frozen
development suites completed with 19/20 accepted; label scores were 14/20 and
11/20, versus historical baseline 20 accepted each and 14/20 and 12/20. Literal
copied goals fell to nine each, but finite transfers/authority requests gained
incorrect continuous labels and one case per suite failed clarification
consistency. It did not meet the criteria declared before inference.

The experiment and its extra unit test were removed. Production interpretation
source matches the pre-experiment commit; no validator, rubric, sampling/output
limit, permission or default policy changed. The exact rejected appendix is
retained only as a static reproducibility document, not runtime instructions.
See [evidence and prompt/binary hashes](INTERPRETER-QUALITY.md#rejected-decision-checklist-experiment-2026-10-17).

Restored locked/offline workspace/all-target suites pass **714 tests plus one
ignored each**, including no-dashboard. Fresh binaries/examples build; public
preview regression `request-preview-vG7wF4` and the exact baseline prompt hash
check pass, alongside format, diff, frozen scorer, evaluation-argument and source
dependency-boundary checks. No model download/copy, coordinator, workload or
installed/physical-node update occurred. No release/update feed changed.
Default model quality/bootstrap and all other release gates remain unfinished;
this is negative development evidence, not an improvement/default approval claim.

## Unreleased: atomic interpreter and backend choice (2026-10-17)

`models interpreter --activate-backend` and installed-GGUF selection with
`--interpreter --activate-backend` save the exact role/provider together under
the existing config lock. Without the flag, provider/default wire behavior is
unchanged. A role or cache failure cannot partially change provider. Adapters
also check the saved provider; unsupported/lost/substituted replies cause no
fallback series of writes or automatic retry. Settings remain saved until an
explicit app start/restart, which applies all pending settings. Other-backend
selections remain saved; this does not implement concurrent provider activation.

Both locked/offline workspace/all-target suites pass **714 tests plus one ignored
each**, including no-dashboard builds. Fresh binaries/examples build. Public local
`installed-selection-COTfk0` and approved remote `model-metadata-6b2ZUj` pass new
activation/owner-denial/preservation checks and existing regressions. With the
existing 1.28 GB development cache, `real-installed-selection-R6vXTz` passes full
checksum/selection (17,019 ms), explicit CPU startup (3,234 ms), applied interpreter
verification despite reachable mock Ollama, and public unload with owned PID exit
observed. No model copies/downloads or inference occurred. Format, JS syntax,
source boundary and diff checks pass. See
[contract and evidence](INTERPRETER-SETUP.md#acceptance-and-remaining-setup).

This removes a manual setup/provider trap, not default model-quality/bootstrap,
complete graphical onboarding, physical two-PC, media/peer/cluster, resource/stop
or installer/signing/Defender acceptance gates. No installed app, physical node,
release version, published artifact or stable-only update feed changed.

## Unreleased: correlated installed-model preparation failures (2026-10-17)

Installed-GGUF selection now reports static, typed cache-preparation rejection
before entering configuration saving. Both CLIs create a fresh request ID;
node/coordinator adapters require exact node, echoed command/correlation, static
fields and false write/unknown flags before confirming no write was attempted.
Missing IDs preserve legacy successful-selection compatibility, not this negative
acknowledgement guarantee. Invalid IDs fail before preparation. Correlation is
not durable deduplication or authority. Later snapshot/save errors, lost replies
and stale or malformed reports remain uncertain and are never automatically
replayed. Cache files and existing configuration are not repaired or deleted.

Both locked/offline workspace/all-target suites pass **710 tests plus one ignored
each**, including the no-dashboard build. Fresh binaries/examples build;
`installed-selection-GJEMo2` and `model-metadata-RC5rma` pass actual owner commands,
confirmed local/remote corruption rejection, stale rejection substituted after
a real save, lost-reply no-replay and the existing protocol regressions. Tests
use synthetic cache bytes/mock Ollama in isolated same-PC processes. Format, JS
syntax, source dependency-boundary and diff checks pass. See
[contract and measured scope](INSTALLED-MODEL-SELECTION.md).

No release, model download/load, installed application, physical node, updater
feed or UI changes. Default interpreter quality/bootstrap, first-run automation,
physical/release acceptance, strong termination, media/peer paths and the later
mesh/cluster roadmap remain unfinished.

## Unreleased: explicit interrupted-result reconciliation (2026-10-11)

Origin/EEF-owner `requests recover-result` now reconciles an exact idle
interrupted reviewed attempt from its authenticated executor's retained completed
report. The existing job journal saves a separate bounded recovery receipt;
original grant, request context, assignment, attempt count, actual ACK and
missing/unknown report remain unchanged. Completed errors fail rather than
silently succeed. Exact same-actor command reuse inspects its durable decision
without another remote lookup or checkpoint. Restarts never launch a runner.
Routine status is redacted; explicit text-output inspection includes recovered
output. See [contract and limits](RESULT-RECOVERY.md).

Both admissions share the existing runner/shutdown gate, with no lock held
across remote inspection. Wrong origin/grant/session, stale revisions, competing
abandonment, damaged/non-completed evidence and journal quota/write failure deny
new decisions without changing the saved job. No synthetic ACK, replay,
remaining-step dispatch, lease renewal, legacy fallback or remote-stop claim is
made. Safe continuation and implicit compound input wiring remain open.

The real local disconnect test found that dropping the client connection cancels
in-flight Rust futures, not necessarily external effects. Accepted runtime drop
now best-effort checkpoints null-output uncertainty and releases unfinished output
reservations while preserving consumption. It never retries a failed final write.
Unknown evidence cannot reconcile as completed; crashes/write failure may still
leave only accepted metadata. Strong process/backend termination is still open.

Validation: **385 workspace/all-target Rust tests pass in both feature modes**,
with fresh no-dashboard binaries. New units cover scope/actor/receipt contracts,
real/missing ACK preservation, restart, failed evidence, damaged records, runner
and shutdown admission, source-revision races, competing abandonment, quota
rollback, command flags and dropped-runtime uncertainty. Remaining-step policy
has a unit test, not compound-pipeline acceptance.

`request-review-RJutzG` passes both-role public-command recovery after deliberately
failed coordinator completion checkpoints, recovery-write rollback, dropped HTTP
reply receipt reuse, executor-offline saved-owner inspection, strict API guards,
origin/actor denial, status redaction and no backend replay. A separate real local
disconnect returns unknown, refuses recovery and permits only explicit uncertainty
acknowledgement. The earlier new fixture incorrectly expected unfinished work to
complete across disconnect; it now tests that boundary honestly. Its initial
60-second grant was also corrected to the existing 30-second protocol cap without
widening production limits or test deadlines.

Final baselines pass in `job-shutdown-pUQnYw`, `scheduling-Vkua3P`,
`request-drafts-WuxHKs`, `model-metadata-jtgKVn` and the isolated
`node-credentials-9MMTXJ`. The durable-request regression
now requires the new unknown/null-output cancellation report and still verifies
no replay after restart, abandonment or EEF history removal. Formatter, JS syntax,
backend/browser source-boundary and diff checks pass. The fake-model/headless
fixtures explicitly report `real_inference:false` and `physical_two_pc:false`.

No release version, installer, installed app/physical node, update feed or UI
change is made. Stable-only automatic updates remain unchanged. Legacy execution
migration, default-LM quality/bootstrap, automatic request policies, strong
termination, continuous/direct data paths, model lifecycle, physical acceptance
and signing/Defender review remain release/backlog work, not completed features.

## Unreleased: bounded retained executor reports (2026-10-11)

Granted operations atomically checkpoint bounded final reports/output in the
existing node grant ledger before sending completion. Admission reserves result
space across independent handles before consumption/ACK/effects. Schema 1-to-2
migration preserves old consumed receipts without inventing output/success;
read-only legacy inspection does not migrate. Damaged/future journals and failed
checkpoints fail closed. Cached output is explicit/private, not routine status.

Origin/EEF owner `requests result` reads the exact retained issuance from its
independently authenticated pinned-issuer executor, including after session
replacement. Executor-local `grants result` works offline. Strict bounds,
scope/identity checks and API guards apply; no other origin, issuer or fallback
can fetch it. Receipt commands stay metadata-only. Reads neither change EEF
tracking nor infer, issue/renew grants, retry or reconcile. See
[retained result contract](RETAINED-RESULTS.md).

This is a lost-output evidence/retrieval foundation, not automatic uncertainty
resolution, strong termination, legacy grant migration or physical acceptance.
Versions, installed apps/physical nodes, installers and stable-only update feeds
remain unchanged. Stop old runtimes and back up data before a future upgrade;
older binaries reject schema-2 grant journals.

Validation: **371 workspace/all-target Rust tests pass in both feature modes**,
with fresh no-dashboard binaries. Added coverage checks bounded/correlated final
reports, canonical checksums, changed cached output, atomic write failure,
reservations across independent handles, replay denial, legacy migration and
read-only/no-file-creation behavior. The pinned-TLS native file-write test
deliberately loses the coordinator completion checkpoint, retrieves the exact
node report across restart/session replacement and preserves an owner marker
without re-executing. These are local isolated tests, not physical acceptance.

`request-review-Q83hlH` passes origin/owner/local CLI/API result reads, another
connected origin's denial, strict API guards, routine-output redaction and both
role restarts with unchanged job state and no inference replay. The earlier new
denial fixture reached a stopped peer; reconnecting it tests actual authenticated
EEF refusal without weakening assertions/deadlines. Baselines pass in
`request-drafts-BdBLPW`, `job-shutdown-KRqpnK`, `scheduling-tq7XqD`,
`model-metadata-rROQlm` and `node-credentials-Kez5vL`. Formatter, JS syntax,
backend/browser source boundary and diff checks pass. Fake-model/headless command
tests do not establish real-model quality, physical load or two-PC acceptance.

## Unreleased: explicit grant-aware submission entry (2026-10-11)

`requests submit` and `jobs request-text` let callers save and compile explicit
one-step inputs without model interpretation, JSON proposal authoring or ordinary
RPC execution. The same service/journal/compiler retains untouched original text,
caller template/parameters/routing choices, authenticated origin and frozen bindings.
Requests await separate owner approval and origin execution. No inference, model
download, workload load/reservation, grant or dispatch is initiated by submission.

Source admission and compilation are separate checkpoints. Failed/unconfirmed
compilation may leave an inspectable source; immutable `concrete_input` prevents
same-receipt substitutions, including later compiler/model attempts. A separate
derived compilation receipt supports one retained source/plan across exact retries
and restart, without renewing authority. Old unmarked records retain digest bytes;
new source fields are included in compiled definition digests. APIs/CLI enforce
typed bounds/identity and never fall back to old chat/jobs when unsupported.
See [contract](EXPLICIT-REQUESTS.md).

This is a real grant-aware entry point and **partial migration**, not a
grant-enforced default. Old `generate-text`, raw task/job creation, chat/rules and
sensitive ordinary RPC remain legacy; they still need migration/retirement.
Their existing control/recovery regressions are preserved. UI, versions, installed
apps/physical nodes, installers and update feeds remain unchanged.

Validation: **364 workspace/all-target Rust tests pass in both feature modes**;
fresh no-dashboard binaries, JS syntax, backend/browser source boundary and diff
checks pass. Headless `request-review-3frx6B` exercises exact generic/text admission,
immutable model/routing/prompt choices, concurrent receipts, strict API guards,
inspectable partial compilation, lost replies and coordinator restart. It proves
no backend call before separate owner approval and origin execution, one granted
text call with the original prompt, durable output and no call on later receipt
reuse. Existing regressions pass in `request-drafts-bW3K0P`,
`job-shutdown-mMf9gT`, `scheduling-MyDlSJ`, `model-metadata-52m1ET` and
`node-credentials-ZDrp1e`. These use isolated local nodes and a fake backend, not
real-model quality, physical two-PC load or release acceptance.

## Unreleased: explicit semantic preparation workflow (2026-10-11)

`eefn requests prepare JOB --revision REV --operation-id UUID` now owns the full
read/context/infer/checkpoint/report path in the backend. No dashboard, JSON
method input or coordinator port is needed. It preserves complete review history
and uses the selected interpreter with shared single-capacity, generation and
inference-deadline guards. Existing explicit deterministic compilation remains.

EEF revalidates authenticated origin, immutable context, exact source revision,
full review digest/counts and fresh disclosed vocabulary. Source is checked again
inside the journal edit. A method uses existing scoped compiler/bindings and saves
as awaiting approval; a question saves as an answerable, source-pinned proposal
changing only clarification fields. Checkpointing never approves, loads executor
models or executes tasks; interpretation itself calls the selected model backend.
Method evidence is included in the definition digest; old marker-free definitions
retain their digest bytes. Question lineage and immutable context are checked on
read/restart. Same-receipt conflicts reject instead of replacing retained work.

Saved receipt inspection precedes model selection/capacity and needs no inference,
save or fresh executor lookup. Unconfirmed mutations stay uncertain unless known
not sent. Lost replies and runtime invalidation never trigger automatic replay.
Automatic answer/open triggers, delegated approval, semantic/bootstrap quality,
compound/continuous work, reconciliation, legacy grant migration, physical two-PC
acceptance and other roadmap stages remain unfinished. No stable release claim.
Installed apps/physical nodes, versions, installers and update feeds are unchanged.

Validation: **359 workspace/all-target Rust tests pass in both feature modes**,
with fresh no-dashboard binaries. Added tests cover strict preparation reports,
digest-bound method evidence with legacy digest compatibility, source-pinned
answerable questions, exact read-only reuse, changed context/source denial,
shutdown, restart and retained corrupt-record recovery. Full bounded question
history is validated without recursive prefix expansion. CLI parsing, formatter,
JS syntax, source backend boundary and diff checks pass.

Headless `request-review-1Remyh` exercises actual CLI/API read/context/infer/save
with a fake selected backend: same-node and disclosed cross-node methods, durable
questions and answer-to-plan history, separate approval/no execution, strict
cross-site/extra-field/wrong-node denial, source races, shared capacity, runtime
invalidation, disclosure revocation, lost replies, journal restart and saved receipt
inspection without a selected role or fresh peer. Existing regressions pass in
`request-drafts-yVg2bi`, `job-shutdown-3GQXAQ`, `scheduling-EQ2iEh`,
`model-metadata-qP5yuH` and `node-credentials-napfcT`. A parallel credential run hit
its existing five-second HTTP deadline; the isolated verification passed without
widening deadlines. Cause is not established; no physical load, real-model semantic
or two-PC acceptance is claimed.

## Unreleased: source-bound semantic method candidate core (2026-10-11)

The selected node interpreter can now produce a bounded untrusted method candidate
or clarification from an exact retained review, preserving original text and full
proposal/question/answer history. Source evidence pins job/revision/full digest/
counts and reported backend/model identity; it does not attest inference or semantic
truth. A single authoritative template/action/parameter catalogue is shared with
EEF's existing planner. Candidate hints cannot invent node/model/resource bindings,
permissions, roles, backends or compute limits; EEF validation/approval remain
mandatory. Existing interpretation prompts/output and explicit template behavior
are preserved.

This is a core schema/prompt/inference increment, **not a new user command, saved
plan, automatic trigger or completed semantic compiler**. Fresh context, service
capacity/generation guards, atomic EEF source verification/checkpointing, saved
receipt inspection and durable new-question handling still need integration.
See [contract and remaining workflow](SEMANTIC-PREPARATION.md).
Installed apps/physical nodes, versions, installers and update feeds are unchanged.

Validation: **354 workspace/all-target Rust tests pass in both feature modes**,
with fresh no-dashboard binaries. New units cover the complete history prompt,
strict method/question validation, exact source evidence and fake selected-backend
inference without journal or native effects. Headless `request-drafts-Nhj9oe`,
`request-review-IZkZo2`, `node-credentials-8UgHti`, `job-shutdown-wmSlSj` and
`scheduling-wj2K4l` retain passing results; `model-metadata-I4ZstF` passed on a
fresh verification run after its earlier terminal output was unavailable. These
are local logical-node regressions, not physical two-PC or real-model acceptance.

## Unreleased: origin-node concrete planning commands (2026-10-11)

The input node can now invoke EEF's existing compiler and inspect its own private
plan via `requests compile/plan` and a guarded local command API, without a
dashboard or coordinator port. Shared typed inputs retain old owner file/API
contracts. A simple parameter-free CLI mode needs no JSON file; advanced inputs
still use explicit data. EEF checks immutable origin before reading/compiling,
then filters fresh executor candidates to applied self/directional owner-disclosed
scope before existing capability/model ranking. No hidden-provider fallback,
inference, permission widening, model load/download, approval or dispatch occurs.

Plans use the same journal, revisions, exact receipt reuse, digest and frozen
bindings. The new strict private snapshot includes original review, inputs and
tasks/bindings, not credentials, endpoints, raw adapter bindings, results or grant
authority. Other origins—including the selected executor—cannot inspect it.
Historical own inspection/exact receipt reuse does not need fresh executor lookup
and cannot renew authority. Unconfirmed mutation replies remain uncertain unless
known not sent, including generic rejection after possible checkpointing.
Approval remains a separate coordinator-owner operation. Automatic semantic
planning, delegated owner approval, grant migration and the complete non-coder
workflow remain required; this is a stage 5-6 command/workflow increment only.

Validation: **349 workspace/all-target Rust tests pass in both feature modes**,
with fresh no-dashboard EEF/EEFN binaries. Added units cover pre-ranking scope
filtering, strict origin/receipt/revision/input/view contracts, malformed graphs,
binding changes, duplicate fields, authority widening, file bounds and disconnected
operation without configuration/journal creation. Rejected/corrupt confirmation
cannot manufacture an absent-save claim or trigger a retry; CLI tests cover
required/conflicting input options. Headless `request-drafts-0otUw3` covers actual
node CLI compilation without JSON, private plan inspection, cross-site/extra-field/
wrong-node denial, undisclosed target denial, exact receipt reuse, other-origin
compile/inspection denial, owner compatibility, restart, separate approval and
the existing native grant/nonce/recovery regressions. `request-review-0niux0`
checks origin inspection of a model-bound plan without inference, actual disclosed
peer compilation when the origin lacks that feature, selected-executor inspection
denial, historical lookup after peer withdrawal and rejection of a new compilation
without journal mutation; prior interpretation/model-grant regressions remain green.
Credential isolation (`node-credentials-zgDDCN`), model metadata
(`model-metadata-cFhMm5`), shutdown (`job-shutdown-jaUYww`) and scheduling
(`scheduling-d6lUmN`) pass. Source-boundary, syntax, formatting and diff checks pass;
fixture children closed. These are isolated local logical-node/fake-model tests,
not physical two-PC or real-model semantic acceptance.

Installed apps/physical nodes, versions, installers and update feeds are unchanged.

## Unreleased: clock-independent fresh grant leases (2026-10-11)

Reviewed execution now anchors the coordinator's monotonic lease before durable
issuance. The exact pinned-session node must obtain one fresh nonce-bound challenge
reply for that pending issuance and complete grant digest before durable consumption.
Its local deadline starts at challenge send, subtracting the whole round trip plus
a conservative timer margin. Journal, queue and ACK delays cannot restart the budget;
expired/mismatched/unanswered proof cannot start effects. An ACK without fresh proof
or after issuer expiry cannot confirm acceptance. Cancelled/failed partial node writes
retire the framed session; accepted receipts remain uncertain, never replayed.

This removes independent-PC wall-clock comparison, not local clock-change checks,
owner approval expiry or replay protection. New advertised timing mode is required
for new grants; no old-clock/ordinary-RPC fallback exists. Read-only inspection of
compatible old receipts remains available without reactivating authority. The node's
local clock baseline precedes its durable claim, detecting changes during that write.
No NTP, hardware drift attestation, hard real-time, suspend safety, strong process/
backend termination or safe coordinator cluster is claimed. Physical clock/network
acceptance remains required; see [timing contract](EXECUTION-GRANTS.md#limits-and-honest-termination).

Validation: **344 workspace/all-target Rust tests pass in both feature modes**,
with fresh no-dashboard EEF/EEFN binaries. Added units exercise synthetic issuer
timestamps offset by +/-48 hours, exact nonce/session/digest matching, expired and
widened replies, full round-trip deduction, local clock changes, cancellation
cleanup and partial-write retirement. Expired proof cannot claim a receipt;
expiry while awaiting ACK prevents native file effects after durable consumption,
retains unknown state and rejects a fresh-proof replay. Gateway tests deny unproved
or expired ACKs, stale sessions, unknown fields and second/renewal challenges.
Headless `request-drafts-IYQNGx` verifies real pinned-TLS native cross-node writes,
HTTP/deadline/uncertainty, nonce and receipt recovery paths with the new timing mode.
Fake-model `request-review-N3ShiH` verifies pinned model-grant execution and the
existing interpretation/clarification regressions. Credential isolation
(`node-credentials-OvVJOX`), model metadata (`model-metadata-0TJ5wp`), shutdown
(`job-shutdown-uzq7hr`) and scheduling (`scheduling-rjRq7J`) pass. Source-boundary,
JavaScript syntax, formatting and diff checks pass; fixture children are closed.
These are local logical-node/synthetic clock/fake-model checks, not actual OS
clock adjustments, physical two-PC acceptance or real-model semantic evaluation.

Installed apps/physical nodes, versions, installers and update feeds remain unchanged.
This is a stage 6 reliability increment, not automatic semantic planning, legacy
grant migration, continuous workloads or stable v0.4.0 acceptance.

## Unreleased: explicit model-assisted clarification resolution (2026-10-11)

Node `requests resolve` now owns a command/API-only read/context/inference/save
workflow for a retained origin-owned review and its current recorded answer.
The selected interpreter receives unchanged original text, the current proposal
and all recorded questions/answers as separate untrusted data; prompts are bounded
at 64 KiB without truncating answers. Existing role/backend/finish/output limits,
single-capacity and runtime invalidation remain. EEF saves one new review-only
proposal atomically against source revision, full review digest and exact history
counts, rechecking fresh disclosed capability names. Source lineage is validated
on retained reads/restart. Original identity/text, proposals and answers persist;
no task, plan, approval, grant or execution is implicit. Backend/model metadata
is origin-reported evidence, not semantic correctness or model attestation.

Same-command saved-receipt inspection avoids inference/save and works without an
available interpreter, including after a lost local HTTP reply. It uses a fresh
read receipt rather than pretending it is the original save. Absent retained
evidence can permit a new **explicit** attempt against an unchanged eligible
source; no automatic replay or exactly-once model call is claimed. See
[resolution commands and limits](COMMANDS.md#explicit-clarification-reinterpretation-unreleased-after-alpha6).

Validation: **339 workspace/all-target Rust tests pass in both feature modes**,
with fresh no-dashboard EEF/EEFN binaries. New unit tests cover bounded unchanged
original/QA prompts, strict result contracts, full source lineage and corruption
rejection, origin/revision/digest conflicts, atomic receipt reuse and durable
reopening without executable work. Headless `request-review-NtBv6u` covers
no-answer/stale/invalid-model rejection without saving; exact model prompt QA;
one explicit save; concurrent answer preservation and stale-inference denial;
runtime cancellation; cross-site/widened request denial; saved receipt inspection
while inference is busy; lost local save reply inspection after the interpreter
role is removed; and no implicit tasks/grants. Its earlier pinned model-grant and
two-node disclosure regressions also pass. Durable requests
(`request-drafts-uex9If`), credential isolation (`node-credentials-MHjkh3`),
model metadata (`model-metadata-HHTRx6`), shutdown (`job-shutdown-tH494u`) and
scheduling (`scheduling-qAhyeH`) pass.
These are isolated local logical-node/fake-model checks, not real-model semantics
or physical two-PC acceptance. Source boundary, syntax, formatting and diff checks
pass; fixture children are closed through cleanup.

This removes manual proposal-file authoring for explicit clarification resolution,
not the automatic answer-triggering policy, default model/bootstrap quality gate,
automatic semantic compiler, complete non-coder request-to-execution workflow,
physical reliability or stable release gates. Installed apps/physical nodes,
versions, installers and update feeds remain unchanged.

## Unreleased: cross-node interpretation vocabulary (2026-10-11)

Online request interpretation now obtains a strict, read-only capability-name
context from EEF before inference. Names come from fresh live advertisements of
the input node and peers already authorized by applied directional discovery
grants; default disclosure remains self-only. The input node need not implement
the suggested feature. EEF independently recomputes membership when reviewing,
opening or revising proposals; input/model-supplied names cannot authorize work.
The new lookup contains no user text, peer IDs, model IDs, endpoints or bindings,
has a five-second deadline, and is cancelled/rejected across runtime change.
Offline preview stays local. Saved vocabulary remains historical evidence;
inspection and clarification answers do not require a current executor.

Validation: **336 workspace/all-target Rust tests pass in both feature modes**,
with fresh no-dashboard EEF/EEFN binaries. Headless `request-review-6CbnZu`
uses two independently paired TLS nodes and fake inference to verify actual
prompt vocabulary when the origin lacks the peer feature, default non-disclosure,
cross-node opening/revision, disconnect/reconnect withdrawal, applied revocation
while inference is in flight, independent EEF rejection without replay, unchanged
historical snapshots and local-only offline preview. No cross-node hint implicitly
creates tasks/grants. The fixture also retains its separately owner-approved
model-grant execution regression. Durable jobs (`request-drafts-XF3HnP`),
credential isolation (`node-credentials-lkCREW`), model metadata
(`model-metadata-67Au7p`), shutdown (`job-shutdown-FNIWEs`) and scheduling
(`scheduling-K3XEKg`) pass; fixture children closed through cleanup. Source
dependency, formatting, JavaScript syntax and diff checks pass. These are local
isolated logical-node/fake-model checks, not physical two-PC or real-model
acceptance. Well-shaped unknown hints now fail at EEF; malformed hints still
fail locally. Existing rejection assertions were updated to reflect that boundary.

This is interpretation context, not automatic planning, execution approval,
peer transport, a model bootstrap solution or stable 0.4.0 acceptance. Installed
apps, physical nodes, versions, installers and update feeds are unchanged. See
[cross-node interpretation](INTERPRETATION.md#cross-node-interpretation-context-unreleased-after-alpha6).

## Unreleased: explicit uncertainty acknowledgement and tracking closure (2026-10-11)

Owner/origin `requests abandon` now closes idle interrupted coordinator tracking
only against an exact revision and retained unknown grant, with explicit
acknowledgement that remote effects may continue. The existing job journal saves
a strict decision receipt and authenticated actor class under distinct `abandoned`.
Original input/context, approved bindings, assignments, unknown attempt and any
ACK/report/results remain retained; only unissued tasks are cancelled locally.
This is not success, remote Stop, recovered output, rollback or retry/migration
authorization. Live runners, shutdown, other origins, stale revisions, invented
grants and reused/widened decisions fail atomically. Exact same-actor receipt reuse
inspects without writing, including after restart. Late completion cannot replace
retained uncertainty, and legacy runners cannot revive abandoned authority.

Normal views expose closure/uncertainty hints without private decision receipts or
parameters. Explicit `jobs remove` is separate, never automatic cleanup, and does
not erase node nonce receipts. Removing history loses coordinator receipt recognition
and evidence; it cannot prove no effects or authorize repeating work. See
[command and recovery limits](COMMANDS.md#explicit-tracking-abandonment-unreleased-after-alpha6).

Validation: **333 workspace/all-target Rust tests pass in both feature modes**.
Fresh headless `request-drafts-ADw5AZ` covers both owner/origin CLI decisions,
cross-site/widening/wrong-origin/stale denial, cold/warm restart uncertainty,
deadline-to-unknown, actor-bound duplicate receipts, unchanged grant/task data,
no replay, and nonce retention after separate history removal. Fake-model review
(`request-review-Cb2zJO`), credentials (`node-credentials-ewwFSO`), model metadata
(`model-metadata-i60KEZ`), shutdown (`job-shutdown-vdNCuL`) and scheduling
(`scheduling-mPV61y`) regressions pass. All fixture children closed through cleanup;
source dependency, formatting, syntax and diff checks pass.

This implements the abandon-tracking recovery branch, not full lost-output/
execution reconciliation or strong executor termination. Automatic semantic
planning/bootstrap, legacy-path grant migration, clock/two-PC acceptance,
continuous pipelines, peer/media paths, model lifecycle and stable release gates
remain incomplete. Installed apps/remote nodes, models, permissions, versions,
installers and disabled stable-only feeds are unchanged. These tests use isolated
local processes/fake inference, not physical two-PC or real reasoning acceptance;
no stable v0.4.0, signing or Defender-clearance claim is made.

## Unreleased: credential-only startup without a legacy secret (2026-10-11)

EEF no longer requires an unused shared secret when the owner has explicitly
disabled legacy authentication. The gateway constructs no legacy key in that mode;
independent credential/certificate validation and explicit provisioning remain
mandatory for connections. A missing TLS identity leaves owner commands available,
not an implicit identity or network trust. Default legacy-enabled setup keeps its
existing non-placeholder secret requirement. Explicit legacy re-enablement rejects
atomically without a valid effective secret distinct from node credentials.
Disabled legacy access also clears the local shared-key pairing advertisement;
saved old keys are not automatically erased. See [migration contract](NODE-CREDENTIALS.md).

Validation: **329 workspace/all-target Rust tests pass in both feature modes**.
The real pinned-TLS grant/receipt unit fixture now uses no legacy gateway secret.
Fresh headless `node-credentials-WI587I` proves a cold EEF restart with no config/env
legacy secret, two independent reconnects, unchanged certificate identity, blank
shared-key advertisement and atomic denial of invalid legacy re-enablement.
Granted request/receipt/restart checks (`request-drafts-7vE282`) and fake-model
review/execution checks (`request-review-cc9dfh`) pass. Source boundary, formatting,
syntax and diff checks pass; all fixture child processes closed through cleanup.

This is an authentication/setup reliability fix, not automatic pairing, physical
two-PC acceptance, credential ESP support or completion of stable v0.4.0. The
remaining roadmap/release gates below still apply. Installed apps/remote nodes,
models, permissions, versions, installers and disabled stable-only update feeds
are unchanged. No Defender/signing clearance is claimed.

## Unreleased: authenticated executor receipt inspection (2026-10-11)

Original-node and local EEF-owner `requests receipt` commands now inspect the
exact retained executor grant through the current independently authenticated,
owner-pinned TLS session. Complete scope digests and bounded issuer/node/job/task/
attempt/state/timestamp fields must match. A replacement session can inspect old
evidence but cannot reactivate its lease. Other origins, changed issuers/scopes,
missing/damaged records and unavailable controls reject without legacy fallback.
Local `grants receipt` now validates the same strict metadata and journal owner/version.

The distinct read-only wire query uses reserved control capacity, retained through
the bounded read/response even while workload capacity is full. A partial failed
response retires the framed connection. It dispatches no executable capability,
calls no model and does not create, repair, prune or change a journal. Reports retain
the original context and coordinator revision snapshot, explicitly setting
`execution_requested: false`, `automatically_retried: false`, `reconciled: false`.
Inspection success is not execution success. Consumed/completed cannot recover lost
output or prove external effects stopped; missing evidence cannot prove no effects.
Unknown jobs remain unknown and non-resumable. This supplies recovery evidence,
not the still-required explicit uncertainty resolution policy. See
[command contract](COMMANDS.md#executor-receipt-inspection-through-eef-unreleased-after-alpha6).

Validation: **327 workspace/all-target Rust tests pass in both feature modes**.
Fresh headless fixture `request-drafts-7W3HPD` passes owner/origin inspection of a
different executor, scope matching, unchanged node/job journals, wrong-origin and
cross-site denial, control availability during a saturated HTTP operation, and
inspection of retained uncertainty after EEF restart without replay. Fake-model
review/granted inference (`request-review-omnDyt`), credentials
(`node-credentials-HsWAOc`), model metadata (`model-metadata-0jhX3K`), shutdown
(`job-shutdown-di5Xic`) and scheduling (`scheduling-edXRhi`) regressions pass.
Source backend/browser dependency, formatting, syntax and diff checks pass.
These are local isolated processes/fake inference, not physical two-PC or real
reasoning acceptance. All fixture children closed through cleanup.

Stable v0.4.0 remains incomplete; the release gates listed below still apply.
Installed apps, the physical remote node, model downloads/selections, permissions,
versions, installers and disabled stable-only feeds are unchanged. No signing or
Defender-clearance claim is made.

## Unreleased: explicit reviewed execution with durable receipts (2026-10-11)

Owner and origin-node `requests execute` now activate only an exact retained
approved revision/digest, using the existing durable job lifecycle. Issuance and
its fixed assignment are saved before dispatch; a direct observer checkpoints
matching executor acceptance/completion before confirming them to the caller.
Fresh pinned sessions, providers/resources, exact model/backend/role and local
permissions are revalidated. The driver reserves existing global/node/provider
future budgets, including granted inference, without fallback to legacy RPC,
another executor/model or automatic retry. Input origin and executor may differ.

Logical journal headroom is reserved before issuance for bounded reports and
recovery. Results are retained once; normal job views expose metadata, not raw
parameters or reports. Exact execution receipt retries inspect without writing or
reissuing work. Restart/shutdown marks outstanding attempts unknown, never resumes
them, and legacy run/resume/pause reject reviewed records. Stop prevents new
issuance but cannot claim uncertain remote effects have stopped. Acknowledgement
persistence is not a distributed second permission-to-start or exactly-once effect
guarantee. See [command contract](COMMANDS.md#explicit-approved-request-execution-unreleased-after-alpha6)
and [limitations](EXECUTION-GRANTS.md).

Validation: **322 workspace/all-target Rust tests pass in both feature modes**.
Fresh headless fixtures pass: cross-node approved writes, durable ACK/final reports,
no replay after duplicate commands/restart, and restart during accepted HTTP work
(`request-drafts-LDFngX`); exact model-role withdrawal denial, restoration, one
granted fake inference and node-local output (`request-review-Atnz3o`). Additional
credential (`node-credentials-zh6xgb`), model metadata (`model-metadata-kEhZdk`),
shutdown (`job-shutdown-LsKLVb`) and scheduling (`scheduling-ySihAt`) regressions
pass. These are isolated local processes/fake inference, not physical two-PC or
real model reasoning acceptance. No model was downloaded.

Stage 6 remains partial. Automatic semantic planning, explicit uncertainty
reconciliation, migration of legacy task/chat work paths to grants, stronger
OS/backend termination, aligned-clock physical acceptance, continuous pipelines,
peer/media paths and model bootstrap/lifecycle remain open. Stable v0.4.0 is not
ready. Installed apps/nodes, permissions, versions, installers and stable-only
disabled update feeds are unchanged; no Defender/signing clearance is claimed.

## Unreleased: bounded owner approval without grant issuance (2026-10-10)

EEF now supports `requests approve` through the same backend planning command/API,
without a dashboard or model call. Approval names the exact compiled revision and
definition digest, explicit validity/execution/result bounds and current pinned
issuer. Fresh fixed executors/resources and exact model/backend/role routes must
still match; unsupported/shared-secret routes or wider node limits reject.
The atomic `approved` checkpoint preserves original context/text, review history
and immutable task definitions. It saves an expiry and strict canonical receipt,
not an execution grant, reservation or runner. See
[command contract](COMMANDS.md#bounded-owner-request-approval-unreleased-after-alpha6).

Exact retained receipt retries inspect without a write or renewal, including after
expiry/cancellation. Conflicting approvals cannot replace them. Restart preserves
approval but never activates work; origin-owned Stop cancels unstarted tasks while
retaining the receipt/digests. Legacy execution and pause/resume reject these records.
Normal node views expose only bounded approval state/time, not concrete inputs or
receipt internals. Quota, shutdown and concurrent state changes fail atomically;
damaged approvals remain retained without repair or activation. New development
states require the new build and must not be automatically downgraded.

This completes **explicit conditional owner approval**, not stage 6 as a whole.
Reviewed plans still cannot execute. EEF's durable issuance, persisted executor
ACK/completion, exact reviewed-plan dispatch and uncertainty reconciliation remain
the next integration boundary. Stronger OS/backend termination, continuous local
pipelines, peer/media paths, model bootstrap/lifecycle and physical two-PC/release
acceptance remain open. No stable v0.4.0 or Defender/signing clearance is claimed.

Validation: **312 workspace/all-target Rust tests pass in both feature modes**.
New cases cover strict bounds/receipts, immutable approval/plan data, no legacy
runner after restart, exact duplicate inspection, concurrent approvals/Stop,
cancellation, expiry/time gates, quota/shutdown rollback, damaged-journal retention
and report/CLI authority checks. Fresh API-only pinned-TLS two-logical-node
approval/restart/cancellation checks pass (`request-drafts-3lITf0`), including an
unchanged node grant journal after owner approval. Fake-inference review and
model-plan checks pass (`request-review-eyboRn`), including legacy approval denial
without another model call. These are isolated local fixtures, not physical two-PC
or real model reasoning acceptance. Source dependency, formatting, syntax and diff
checks pass. Installed apps, model selections/downloads, permissions, installers,
versions and stable-only public update feeds are unchanged.

Additional fresh headless regressions pass: independent credentials
(`node-credentials-a1nlGl`), model metadata/selection and legacy durable jobs
(`model-metadata-lMD3WF`), shutdown (`job-shutdown-MTCaxi`) and scheduling
(`scheduling-9NXLU1`). All fixture child processes were closed by their cleanup.

## Unreleased: pinned-session node grant consumption and receipts (2026-10-10)

The node has a distinct `execute_grant` path that requires independent exact-node
credentials, an owner-pinned TLS issuer and matching fresh connection generation.
Strict scopes bind the original context, job/task/attempt, plan and grant digests,
one operation, executor/resource/model/backend/role and bounded lease/result policy.
It never falls back to shared-secret or ordinary RPC execution. Existing node
permissions remain necessary; relevant saved revocation/scope changes deny pending
or new granted work without widening the applied runtime.

SQLite consumption is atomic and durable before acceptance/effects. Grant UUID and
stale/duplicate/unresolved attempts cannot be replayed, including through independent
journal handles or node restart. Unknown/damaged journals are retained, not repaired;
quotas fail before effects and no receipt pruning is automatic. The gateway correlates
the exact durable acceptance before confirming completion, rejecting forged/stale ACKs,
unpersisted success, ordinary RPC replies and attempts to undo acknowledgement.
`eefn grants receipt` and the guarded local API inspect existing data read-only without
starting a runtime, creating a journal, exporting parameters or replaying work.
See [scope, storage and honest deadline limitations](EXECUTION-GRANTS.md).

This is the node-side stage 6 boundary, **not the complete approved workflow**.
Normal reviewed requests still stop at `awaiting_approval`. Owner approval, EEF's
durable issuance/ACK lifecycle and reviewed-plan dispatch/reconciliation remain pending.
Legacy execution is unchanged, not retroactively grant-enforced. Deadlines currently
bound the RPC future, not all OS/backend side effects or process trees. Full physical
resource enforcement, pipelines, peer paths and cluster fencing remain later gates.
No stable v0.4.0, installed-node update, installer/signing or Defender clearance is claimed.

Validation: **303 workspace/all-target Rust tests pass in both feature modes**.
New tests run actual pinned-TLS client/gateway file-write execution and verify matching
acceptance/completion, unchanged owner markers after replay and node restart, 24 racing
claims through separate journal handles, immediate saved revocation before effects,
scope/clock/expiry failures, quotas, damaged/future journal retention, canonical scope
digests and read-only receipt/schema/CLI checks. Source backend/browser-boundary,
formatting and diff checks pass. These are isolated local fixtures, not physical two-PC
or full termination/release acceptance. Installed apps, model selection/downloads,
feature permissions and public update feeds are unchanged.

Fresh headless regressions pass: pinned-TLS draft/compile/restart and guarded
read-only receipt checks (`request-drafts-BwV4vj`), fake-inferred review
(`request-review-ZeLdD9`), credentials (`node-credentials-F8zv58`), model
selection/jobs (`model-metadata-CQs9u2`), shutdown
(`job-shutdown-JNtYhF`) and scheduling (`scheduling-WLYMXF`). No browser or physical
node was used by these fixtures.

## Unreleased: concrete reviewed plans awaiting approval (2026-10-10)

EEF now offers owner command/API `requests inspect/compile` without a browser.
Strict explicit template inputs compile resolved one-shot reviews against current
registered providers; model selections must match fresh gateway metadata, including
roles/readiness rather than only cached model IDs. Compilation preserves immutable
original text/context and proposal/answer history, rejects capability widening,
unused inputs and unsupported routing, and freezes exact node/model/backend/resource
bindings with zero automatic retries. It reuses the existing task planner and job
journal, not a second execution framework. See [command contract and limits](COMMANDS.md#concrete-request-plans-awaiting-approval-unreleased-after-alpha6).

The atomic checkpoint is `awaiting_approval`, with a SHA-256 definition digest,
**not execution authority**. Restart preserves it without a runner; execution and
pause/resume reject it, and origin-owned stop cancels tasks without changing their
definition. Strict receipt/revision/runtime checks prevent implicit replacement.
Exact retained compile/opening receipts deduplicate without rewriting. Full owner
inspection is private; normal node job/draft views omit concrete inputs/bindings.
Quota/shutdown failures leave the original review unchanged; damaged definitions
are retained for recovery, not repaired or activated. New journal states require
the new build; no automatic downgrade is safe.

This advances stage 6's **explicit concrete compilation**, not automatic semantic
planning, approval, signed execution grants, executor acknowledgement, node-side
issuer/attempt replay fencing, continuous pipelines or safe cluster failover.
The digest is not a signature or defense against a database owner. Legacy task/chat
execution is not silently changed into a grant-enforced workflow. Current provider
availability is a planning hint; future dispatch must revalidate and authorize it.
Stable v0.4.0 is not ready or published by this checkpoint.

Validation: **292 workspace/all-target Rust tests pass in both feature modes**,
including non-executable restart/cancellation, immutable definitions and context,
opening-receipt reuse after compilation, malformed report/input rejection, capability
widening denial, UTF-8/BOM and size limits, quota rollback, shutdown rejection,
damaged-journal retention, current-versus-cached model metadata, and CLI syntax.
Fresh headless fixtures pass: pinned-TLS two-logical-node compile/restart/stop
(`request-drafts-46mhzg`), fake-Ollama model-role binding without inference
(`request-review-SPgvow`), credentials (`node-credentials-OWrHrR`), shutdown
(`job-shutdown-dxpx5w`), scheduling (`scheduling-RO1Ft9`) and model/job selection
(`model-metadata-J6sfXx`). Formatting, syntax,
diff and source backend/browser-boundary checks pass. These are isolated local
processes, not physical two-PC or real model reasoning acceptance.
Installed apps, permissions, models, installers and public update feeds are unchanged.

## Unreleased: gateway-session-bound reply ownership (2026-10-10)

RPC waiters now retain the exact authenticated connection generation, not just
a stable node ID. Registration of a waiter holds the routing read guard until
its pending entry exists; route retirement then drains only that session's
waiters. Response completion checks both identities. Old cleanup cannot fail
a replacement session's request, nor can the replacement confirm an old waiter.

Accepted submissions get a gateway-owned routing generation, replacing any
caller field. Completed/busy EEF replies require that original live session and
matching transport ID; they never follow the node ID onto its latest connection.
Missing, malformed, stale generations and saturated queues reject without late
delivery or replay. No wire requirement, persisted identity or job origin changes.
See [reply semantics](COMMANDS.md#reply-ownership-across-reconnect-unreleased-after-alpha6).

This is ephemeral response ownership and retirement integrity, not durable
issuer epochs, an execution grant, node-side attempt fencing, exactly-once effects
or remote cancellation. Work accepted before disconnect may still be saved/running;
the existing uncertain-receipt inspection path remains mandatory.

Validation: **284 workspace/all-target Rust tests pass in both feature modes**.
New tests reject cross-session completion/cleanup, preserve replacement waiters,
serialize waiter insertion with cleanup, replace forged generation fields and
exercise an actual TCP reconnect that deliberately reuses a transport ID.
The real gateway refuses to reroute a previous completion and delivers only the
current session's reply. Headless pinned-TLS draft/credential, fake-inferred
review/model-job and shutdown fixtures pass (`request-drafts-F8UCJq`,
`node-credentials-Vu7iw9`, `request-review-bf0g4i`, `model-metadata-BQP1Ap`,
`job-shutdown-4OEs6b`). Formatting, diff, syntax and source backend/browser-boundary
checks pass. No installed-node, permission, model download, installer or public
update-feed change; no physical two-PC or stable-release acceptance claimed.

## Unreleased: durable origin-owned request review (2026-10-10)

Nodes now have command/API-only `requests open/open-proposal/draft/answer/revise-proposal`
operations. Optional natural-language opening reuses the bounded interpreter;
supplied proposals, inspection and answers need no model. EEF saves non-executable
`review` records in the existing durable job journal with stable identity, exact
original text/context, proposal history and clarification answers. Original
origin/area/text are immutable, explicit revisions protect concurrent edits, and
retained operation receipts deduplicate exact openings/answers/proposals without
rewriting checkpoints. Revision overflow now rejects rather than reusing a number.
See [commands and privacy/bounds](COMMANDS.md#durable-request-review-and-clarification-unreleased-after-alpha6).

Only the authenticated origin node can access the full record; ordinary job views
expose bounded state/counts instead of raw review content. Existing journal quotas
and recovery headroom apply. Restart retains review state without admitting a
runner. Pause/resume/execution entry points reject it; stop cancels it, and removal
is explicit. Corrupt review state is retained for recovery, not silently rewritten
or activated. CLI/service confirmation checks origin, receipt, identity, requested
delta, revision and false execution flags. Lost replies never cause replay; receipt
lookup finds an uncertain opening without another inference or save.

An answer records a fact, not a model resolution or approval. Even a confident
revised proposal still requires review. This advances stage 5's durable clarification
foundation, **not** authoritative planning, owner approval, execution grants,
executor acknowledgement, node-side replay fencing or stable-release acceptance.
The receipt protection lasts only while that record remains in the journal.

Validation: **280 workspace/all-target Rust tests pass in both feature modes**.
New coverage includes 24 racing opens/answers, retained-history deduplication,
stale/conflicting edits, wrong-origin denial, journal reopen, quota rollback,
damaged-state retention, non-executable entry points, strict report/bounds checks,
runtime invalidation and lost/corrupt mailbox replies without replay. Headless
two-logical-node pinned-TLS fixture `request-drafts-rXjH4Z` passes without models;
fake-Ollama fixture `request-review-LGzXQc` verifies inferred review and receipt
lookup after a deliberately dropped save reply. Credential and shutdown
regressions pass (`node-credentials-91Y1Dm`, `job-shutdown-5y5Bxh`), as do model/job
and scheduling checks (`model-metadata-LYlSYh`, `scheduling-XU717q`); formatting,
syntax, diff and source backend/browser-boundary checks pass. These are isolated
local processes, not physical two-PC acceptance or real model reasoning.
Installed apps, permissions, models, installers and public update feeds are unchanged.

## Unreleased: one executor assignment per durable attempt (2026-10-10)

The journal now admits at most one executor assignment for a durable task
attempt, under its existing store lock. Identical duplicates, conflicting
executors/backends, stale attempts and no-longer-active jobs fail without
changing the checkpoint. A policy-permitted retry needs a new recorded attempt.
Current claimed batches still follow existing pause/stop semantics.

Durable model jobs select one provider per attempt rather than traversing a
fallback chain inside it. Model assignment records include the exact selected
node/backend/model ID. Existing records without a backend remain unknown;
history is neither guessed nor rewritten. Assignment happens before the remote
invocation; a journal error does not mark an unattempted backend unavailable.
Legacy non-job interactive chat retains its existing fallback behavior.
See [job command semantics](COMMANDS.md#added-in-v040a3-jobs-through-the-running-node).

This is coordinator journal integrity, not an execution grant, node-side durable
replay fencing, acknowledged remote stop, expiry or safe active-work migration.
Existing retry-safe policy remains in effect; a local attempt ending does not
prove remote work stopped. Those remaining stage 6 authorization boundaries
are still required before stable-release acceptance.

Validation: **268 workspace/all-target Rust tests pass in both feature modes**.
New tests race 24 assignment writers against one attempt, reject identical and
conflicting reassignments/stale attempts atomically, retain unknown legacy
backends, preserve selected backend after real database reopen without replay,
and prove one journaled model failure leaves the next provider unassigned and
available until a separately recorded attempt. Headless fake-Ollama fixture
`model-metadata-op6vDJ` confirms the backend/node/model assignment through the
real API and retains model/selection/job/pause/restart regressions; it is not
real inference. Shutdown recovery passes (`job-shutdown-YL3xTP`). Formatting,
syntax, diff and source backend/browser-boundary checks pass. No installed-node,
model download, permission, installer or public update-feed change.

## Unreleased: exact remote identity enforcement (2026-10-10)

A real gateway regression reproduced a legacy-key bypass in the unreleased
credential implementation: policy lookup used the supplied ID, but signature
verification then trimmed it. A whitespace alias of a protected/revoked ID could
therefore authenticate with the network key. The gateway now validates the exact
remote ID before policy lookup and verifies those same bytes without trimming.
Invalid claims do not reserve an identity, register providers or consume a replay
receipt. Replay keys are typed `(node ID, nonce)` pairs, not delimiter strings.

Remote IDs also cannot use the query wildcard or EEF's internal `local` provider
alias. Provisioning/revocation, credential policy, TLS selector, invitation
pairing, node client, saved settings, first-run identity and peer registration
enforce the same rule. Existing valid IDs remain case-sensitive and unchanged;
display names/coordinator advertisements are separate. Invalid existing IDs are
not silently renamed or overwritten. See [migration boundaries](NODE-CREDENTIALS.md).

Validation: **264 workspace/all-target Rust tests pass in both feature modes**.
The gateway test failed on the old implementation with an actual `ok` reply for
a whitespace-protected ID, then passes after the fix. Coverage includes protected/
revoked aliases, Unicode boundary whitespace, reserved IDs, control characters,
unchanged replay/connection state after denial, delimiter-distinct nonce pairs,
both client auth modes, policy/provisioning atomic rejection, registration and
byte-identical startup rejection. This is identity hardening, not new execution
grants, remote cancellation acknowledgement, installer signing or stable-release
acceptance. Headless credential, scheduling, node-command and review regressions
pass (`node-credentials-GuOmhh`, `scheduling-X2kKqj`, `node-commands-bvleei`,
`request-review-UfPjE3`); formatting, diff and source backend/browser-boundary
checks pass. Installed nodes and public artifacts/feeds are unchanged.

## Unreleased: command-owned coordinator scheduling budgets (2026-10-10)

The local EEF owner now has typed `scheduling show/limits/node/remove-node`
commands for non-model freshness and global/per-provider/exact-node concurrency
ceilings. Validated, atomic edits preserve latest unrelated configuration and
report saved/applied separation. The running registry uses immutable applied
policy; an explicit coordinator restart is required. Generic settings clients
cannot drop or replace this command-owned policy. Zero preserves compatibility
by removing an additional ceiling, not executor limits or local permissions.
See [command contract and bounds](COMMANDS.md#coordinator-scheduling-limits-unreleased-after-alpha6).

Global/exact-node budgets now retain reservations for removed/replaced providers
until the owned future ends. Retired futures remain observable; late release
cannot decrement a replacement. Read-only selection and atomic dispatch use
the same effective limits. Owner inspection bounds node/provider collections
and omits parameters, context, secrets and dedicated model inference providers.
CLI checks report identity, safety flags and the requested saved delta; corrupt
or lost replies do not cause retry or fallback. Runtime/schema/cross-site/extra
field checks reject unauthorized or ambiguous command mutations.

These are coordinator-local non-model future budgets, **not** physical resource
limits, remote cancellation acknowledgements or execution grants. Dedicated
model routing and direct owner diagnostics/recovery remain separate. This
supersedes the previous entry's pending owner scheduling-configuration item,
not the remaining planning/grant/runtime acceptance work.

Validation: **259 workspace/all-target Rust tests pass in both feature modes**.
New coverage includes concurrent aggregate admission, retained retired-future
accounting, executor-limit intersection, configurable freshness, strict policy
bounds, byte-identical no-op saves, CLI syntax and report confirmation. Headless
two-logical-node TLS fixture `scheduling-dunSG5` proves saved limits leave the old
runtime unchanged, restart applies them, full budgets block scheduling while
owner diagnostics remain available, finished requests release capacity, stale
generation/cross-site/extra fields fail, generic saves preserve limits, and a
corrupted persisted save is unconfirmed without replay. No model/job is created.
Credential, node-command, review, scoped-discovery and shutdown regressions also
pass (`node-credentials-HA0qhS`, `node-commands-XDYHnp`, `request-review-zcU9x2`,
`peer-discovery-pZKAqT`, `job-shutdown-pyAksO`); formatting, syntax, diff and source
backend/browser-boundary checks pass.
Physical two-PC acceptance is not established; installers, installed nodes,
models, feature permissions and release feeds are unchanged.

## Unreleased: atomic provider reservation and truthful freshness (2026-10-10)

Non-model dispatch now chooses its executor and increments coordinator in-flight
accounting under one registry lock. Owned reservations release on every local
exit, including cancellation, journal assignment failure and disconnected remote
requests. Explicit registration replacement, node removal and removed capabilities
retire old reservation IDs; late releases cannot decrement their replacement's
counter. In-place refresh preserves reservations for unchanged capabilities.
Locality/resource/health/priority selection remains shared with read-only lookup.

Stale non-model providers are excluded rather than merely ranked last. Gateway
registration/heartbeat snapshots carry coordinator-stamped receipt times; a
node cannot forge them and cached reconciliation cannot make old state fresh.
Known target/hardware constraints with incorrect types cannot silently widen
selection; unknown VRAM does not satisfy a positive minimum. Malformed constraint
containers are rejected before resource merging. Dispatch checks the selected
remote provider's parameter schema, and local adapters check their own schema,
not whichever provider happens to appear first in inventory.

These reservations bound **coordinator-local in-flight futures**, not physical
CPU/RAM, remotely continuing effects, execution grants or remote acknowledgements.
`max_concurrent: 0` still means no per-provider ceiling. Remote node admission
limits remain a separate service. Model inference uses its existing dedicated
route and has not gained these reservation/freshness guarantees. The structured
review adapter remains review-only; authoritative plan/grant issuance is pending.
Owner command-based coordinator scheduling-limit configuration is still pending;
the existing node request-limit commands are not a coordinator provider policy.
No new visual UI, installed-node change or release is implied.

Validation: 251 workspace/all-target Rust tests pass in both feature modes.
New tests hold 24 simultaneous callers against one slot, verify release after
cancellation/journal failure/offline dispatch, prevent late-release interference
after replacement/removal, preserve in-place refresh accounting, reject malformed
targets and prove cached reconciliation cannot renew receipt freshness. Existing
gateway snapshot tests reject forged timestamps while retaining sanitized state.
Headless integration evidence: `node-credentials-3mzw6W` validates real non-model
dispatch over TLS, malformed-target rejection and released counters;
`peer-discovery-jwkW2p` verifies stale-node exclusion with fresh-node dispatch and
direct owner diagnostics still available. Command/review/shutdown regressions
passed (`node-commands-rZeTiE`, `request-review-ORwzfX`, `job-shutdown-8vfCHe`).
Formatting, syntax, diff and source backend/browser-boundary checks pass. Test
fixtures contain only isolated logical nodes and do not establish physical
two-PC acceptance. No installed-node, model, permission or release-feed change.

## Unreleased: owner-pinned TLS credential transport (2026-10-10)

Independent credentials now require standard Rustls/Tokio Rustls TLS 1.3 around
the existing encrypted frames. First explicit provisioning creates a persistent
coordinator certificate/private key; schema-2 invitations contain only the public
certificate and exact-node secret. Nodes use only that owner-paired trust root and
additionally compare the verified leaf bytes. There is no public-root pool, TOFU,
insecure verifier, TLS 1.2, early data, resumption, silent certificate replacement
or credential downgrade. Existing ring dependencies are reused; rcgen generates
the self-signed identity within the workspace's Rust 1.88 requirement.

Coordinator restart/provisioning another node/node-key rotation preserve its TLS
identity. Status distinguishes `pinned_tls13` from `legacy_shared_key`; inspection
shows a public fingerprint and redacts node secrets and the coordinator private
key. Generic coordinator saves preserve the full latest credential/TLS policy;
remote node settings cannot replace trust. Selector/ClientHello and probe/auth
read-ahead bytes are retained, including coalesced and fragmented delivery. Gateway
shutdown owns incomplete TLS handshakes, not just authenticated sockets.

Earlier **unreleased** schema-1 credential builds require explicit owner rotation
and pairing again; raw credential frames are refused even with the correct key.
A policy without a TLS identity cannot accept credential sessions; local owner
controls remain available for provisioning/recovery. Legacy shared-key behavior
for other identities remains compatible until explicitly disabled. See
[migration and security boundaries](NODE-CREDENTIALS.md).

TLS record replay is rejected within/across sessions. This is not persisted
logical-command/attempt replay protection, execution grants, stale-leader fencing,
remote cancellation acknowledgement or an authorized peer data service. The TLS
certificate is not Windows installer code signing. Physical two-PC migration,
external security review and release acceptance remain pending.

Validation: **243 workspace/all-target Rust tests pass in both feature modes**.
Tests cover wrong/missing pins, wrong keys/IDs, schema-1 and raw-frame downgrade,
TLS 1.2/ALPN denial, malformed/bounded/mismatched certificate material, same/cross-
session ciphertext replay, 64 KiB fragmented streams, coalesced read-ahead and
partial-handshake shutdown. Headless public-command two-logical-node fixture
`node-credentials-AJUEB3` confirms persistent identity across restart and rotation,
disabled-legacy reconnect, deferred revocation, private-key redaction, preserved
permissions and no-model structured review. Existing command/review regressions
passed (`node-commands-y49XEl`, `request-review-gMrzWH`); formatting, syntax and
source backend/browser-boundary checks pass. Scoped discovery and job-shutdown
regressions also passed (`peer-discovery-VvHb02`, `job-shutdown-KsdcfP`). No installed node, model download,
permission, release feed, installer, signing or public artifact was changed.

## Unreleased: model-independent structured review adapter (2026-10-10)

`eefn requests review-proposal --file` sends a supplied structured proposal to
EEF for review without consulting an interpreter. The strict bounded UTF-8
file contract preserves original text; CLI checks shape, the live node runtime
checks capabilities, and EEF independently revalidates gateway-stamped context.
Both model-generated and supplied hints use the same confirmed review service.
The supplied adapter additionally compares its exact proposal in the final CLI
reply and labels `interpretation_source: supplied_proposal`.

This is an optional automation input adapter, not required for ordinary setup.
It never picks/downloads/loads a model, authorizes a capability, dispatches work
or persists a job. Files cannot supply origin, permissions, commands or grants.
Clarification questions are returned without guessing an answer. Restart invalidates
pending review; failed/lost/corrupted replies never trigger fallback or replay.
Interactive clarification/approval, authoritative planning and execution grants
remain unfinished. See [command contract](COMMANDS.md#supplied-structured-proposals-unreleased).

Validation: 236 workspace/all-target Rust tests pass in both feature modes.
Unit tests cover strict file/authority rejection, no-model review with inference
capacity occupied, trusted capability context, original text, confirmed replies
and no replay across runtime retirement. Headless two-logical-node integrations
confirm independent-key review with zero selected models, clarification output,
wrong-ID/capability/schema/cross-site denial and empty job journals. The rebuilt
existing review regression (`request-review-lJPtPR`) also verifies that a proxy
changing a supplied proposal's goal to another schema-valid value is rejected by
CLI without invoking a model or replaying. Formatting, syntax and source backend/
browser-boundary checks pass. No installed-node, permission, model, release-feed,
installer, signing or public artifact change; physical acceptance remains pending.
Latest combined credential/model-free review evidence: `node-credentials-KSWbSF`.

## Unreleased: independent exact-node credential foundation (2026-10-10)

Local-owner `eef credentials` commands now provision, inspect, rotate and revoke
an exact stable node identity and explicitly enable/disable legacy sign-in.
`eefn connection pair --code-file` applies a bounded private invitation through
the running node service, preserving ID, permissions, models and connection-enabled
preference while requesting an app restart. It accepts UTF-8/BOM and BOM-marked
UTF-16 redirected command output. No JSON configuration editing or browser is
needed for this workflow. The invitation is a reusable secret, not a one-time token.

The gateway selects one independently generated key using a bounded public
node-ID selector. Decrypted identity must match it. Wrong/unknown/revoked keys
cannot fall back, reserve identities or poison another identity's replay cache.
Provisioned/revoked IDs also cannot authenticate with the shared network key.
Revoked identities retain tombstones; replacing a credential requires explicit
rotation. Legacy compatibility remains the default for other identities until
the owner disables it. Saved changes are not immediate revocation: coordinator
restart is required to retire old sessions/apply keys. Local automatic pairing
cannot silently replace independently configured trust.

Status/config views redact credentials. Generic coordinator configuration saves
preserve the latest credential policy; remote node settings cannot rewrite its
trust or ID. Current-runtime command checks, existing local API request guards,
independent-key/size validation and atomic config edits apply. Authentication
timestamps reject extreme values without overflow. The existing replay cache is
bounded and duplicate attempts no longer refresh receipt lifetime. Bounded legacy
timestamp nonces from v0.3.2 remain compatible.

This reuses existing cryptography/framing. It does **not** establish session
message-replay fencing, forward secrecy, execution grants, acknowledged remote
cancellation, separately authorized peer data paths or cluster safety. See
[credential migration and limitations](NODE-CREDENTIALS.md). Physical two-PC
migration/rollback and reviewed issuer-session security remain required gates.

Validation: 233 workspace/all-target Rust tests pass with and without dashboard
features. Protocol tests cover independent keys, cross-identity and shared-key
impersonation denial, revoked/unknown IDs, explicit legacy denial, bidirectional
encrypted registration/control, shutdown cleanup, replay bounds and old nonces.
Private-file/config tests preserve owner policy and reject malformed/oversized
invitations. Command-based two-logical-node fixtures passed in default and
headless builds, including disabled-legacy reconnection, deferred revocation,
explicit rotation, protected config saves, private-code redaction and PowerShell
file encodings. These are isolated one-PC fixtures, not physical acceptance.
Existing node-command, scoped discovery, request-review and job-shutdown fixtures
also passed; no model download, installed-node migration, permission change,
release-feed change, installer/signing operation or publication was performed.

Latest headless command evidence: `node-credentials-SJw2Zc` and
`node-commands-Cjv77Z`. Earlier regression runs: `peer-discovery-zGryU9`,
`request-review-qyExIK` and `job-shutdown-FGZnrw`. Fixture paths are beneath
the ignored `.validation/` folder and contain test-only private credentials;
never publish those directories as diagnostics.

## Unreleased: reserved-capacity remote restart workflow (2026-10-10)

The bounded control pool initially protected raw status/restart requests, but
EEF's restart command first requested full configuration and diagnostics through
work capacity. Updated nodes now include a versioned control snapshot in existing
status replies: approval state, stable node ID, current runtime ID and connection
state. EEF uses reserved status for preflight and completion polling, avoiding
full-configuration retrieval over the network and work-slot dependence. The node
still rechecks owner approval when applying restart. Control slots are bounded;
confirmed rejection reports not-started, without retry or an uncertain execution
claim. Saturating all controls still prevents immediate restart.

Older same-ID status replies without the new schema retain read-only get/diagnostic
compatibility. Versioned malformed/mismatched replies, capacity rejections and
transport failures do not fall back or send restart. No permissions are enabled
by inspection, and interrupted inference is never automatically resubmitted.

Validation: 225 workspace/all-target Rust tests passed in both feature modes.
The rebuilt headless model/control fixture (`model-metadata-KXp27q`) holds the
only work slot, then verifies both denied and approved full restart commands.
Approval denial sends no restart; approval confirms the same stable node with a
new runtime ID, without replaying interrupted inference. Unit tests cover bounded
status projection, strict schema/identity/runtime validation, legacy inspection,
and confirmed preflight/restart capacity rejection without retry. Formatting,
diff, JavaScript syntax and browser-boundary checks passed. This uses isolated
one-PC processes and fake inference, not physical two-PC validation. No installed
node, model, permission, release-feed, installer or signing change.

## Unreleased: event-loss recovery and observer survival (2026-10-10)

A deterministic overload test evicted registration/disconnect events from the
gateway's 256-event channel and reproduced stale/missing scheduler entries.
The gateway now retains only each live session's latest sanitized registration
and metrics, with no request history or adapter bindings. EEF reconciles providers,
models and node observations at listener start and after lag. State notifications
consult live snapshots so old disconnects cannot delete a newer session.
Normal heartbeats clone only metrics, without revalidating all advertised models.
Scheduler-owned counts/limits and local adapters survive refresh. Metrics-only
disconnected nodes are also removed. Recovery updates observations directly;
it does not replay lifecycle triggers or lost submissions into automation.

World and assistant observers previously exited permanently on broadcast lag.
They now keep consuming later events and expose dropped-event counters. World
observations conservatively retain `state_may_be_stale` after a gap; automatic
world-condition rules suspend while that flag is set. Explicit commands and
separate event/keyword rules remain available. This flag lasts for the world
runtime's lifetime; reconnect or node reconciliation cannot prove that missing
application/sensor/task observations were restored. No automatic work replay,
execution lease or full world-state reconstruction is claimed.

Validation: 222 workspace/all-target Rust tests passed with and without dashboard
support. Gateway-overload and world-observer regressions failed before their
fixes; the final gateway test verifies startup/lag reconciliation, live provider
and model recovery, metrics-only cleanup, local adapter/counter preservation,
observer repair without synthetic connection triggers, and no lost request replay.
Assistant tests verify resumed listening and suspension of stale world rules.
Sanitized snapshot tests cover latest state, independent copies and secret/binding
redaction. Formatting, diff, JavaScript syntax and browser-boundary checks passed.

Rebuilt API-only process fixtures passed request review (`request-review-Z8RKQG`),
local preview (`request-preview-TLS7o4`), node commands/diagnostics
(`node-commands-lFybtB`), shutdown/recovery (`job-shutdown-W21ICe`), scoped
two-logical-node discovery (`peer-discovery-G9JUZN`) and model/job/limits controls
(`model-metadata-SbnZB8`). These are isolated one-PC fixtures with fake inference,
not a physical two-PC acceptance run. No installed node, model, permission,
release-feed, installer or signing change.

One combined capacity-one model fixture (`model-metadata-Ukr9mW`) received an
unconfirmed multi-stage model-setting reply after reconnect. Capacity rejection
was previously classified as uncertain at this boundary. EEF now preserves a
confirmed `NodeBusy`/`not_started:true` result as `node_busy`, with no automatic
mutation retry; missing confirmation remains uncertain. Tests cover rejection
both before and after preflight. The successful fixture verifies real saturated
preflight rejection without saving configuration, then restores the normal work
budget for unrelated model-control checks. A capacity slot remains occupied
through reply handling; receiving one reply does not guarantee immediate spare
capacity for another command.

## Unreleased: explicit node-to-coordinator request review (2026-10-10)

`eefn requests review` extends bounded local interpreter preview through the
existing authenticated node submission path. The gateway overwrites origin and
capability context using accepted registration snapshots; EEF revalidates the
strict schema, bounds and hint membership. Both service and CLI confirm response
identity, original text and review-only flags. EEF returns review/clarification
state without invoking its brain, planning, dispatching or writing a job.

Review is ephemeral, not a durable receipt or approval. It has no automatic
retry, default-model selection or chat fallback. The one-inference preview
budget is shared; restart invalidates waits. The configured external model may
continue computation after local cancellation. Local preview remains usable
with EEF paused. Interactive clarification and independent execution grants
remain planned; a model's well-formed proposal is not proof of understanding.

Validation: strict-schema Rust tests and isolated headless request review
(`request-review-MriG3f`) and preview (`request-preview-kg5qj1`) fixtures passed.
Cases cover exact original text, gateway-stamped origin/area, clarification,
invalid authority fields, shared inference capacity, restart invalidation,
offline EEF, lost/corrupted replies and missing older endpoints without fallback.
No jobs were created. Final combined verification is recorded above when complete.
These use fake inference on one PC, not physical two-PC validation. No installed
node, model, permission or release change.

## Unreleased: bounded node request admission (2026-10-09)

Incoming node requests previously spawned an unbounded number of execution
futures. A shared node-client budget now defaults to 16 work requests and two
separate control requests, configurable through validated `request_limits`.
Control capacity is limited to ping and node status/restart; restart approval and
all other normal permission checks remain in the executor. Excess work receives
`NodeBusy`/`not_started:true` without spawning an execution future. No backlog or
automatic node replay is added. Permits last through execution/reply handling and
are released on completion or cancellation, including connection teardown.

Diagnostics expose sanitized applied-limit snapshots and a busy-rejection counter.
Older configs receive defaults. Invalid owner/approved-remote saves must not replace
valid saved configuration. These are network request-future limits, not limits on
CPU/RAM, local API actions, background downloads or effects surviving cancellation.
The roadmap's full workload runtime and independent execution grants remain planned.

`eefn connection limits` inspects or saves work/control values through the shared
node command service, without JSON editing, a browser, a second runtime or an
automatic restart. Atomic partial updates preserve unrelated configuration and
no-ops do not rewrite it. Reports distinguish saved from applied limits.

Validation: 212 workspace/all-target Rust tests passed in both feature modes.
Rebuilt headless processes passed the model/job/limits command fixture
(`model-metadata-Z2dWdz`): invalid saves leave configuration unchanged, partial
updates preserve other fields, saved/applied values differ until restart, and
setting limits does not restart the runtime. Saturated work rejects without
starting inference while ping/status and permission-checked restart remain
responsive. These are isolated local fixtures with fake inference, not physical
two-PC validation. No installed-node, model, permission or release change.

## Unreleased: saturated reply queues fail promptly (2026-10-09)

A regression reproduced `send_to_node` waiting indefinitely behind a full
outbound queue. Besides job replies, the shared coordinator event listener uses
this path to reject excess submissions; one blocked node could therefore stall
other registration/heartbeat/disconnect processing. Reply admission now uses the
existing bounded queue without waiting, reports queue-full separately from an
offline node, and never schedules rejected replies for later delivery. Ordinary
remote invocation retains its existing bounded send/reply deadline.

Queue admission is not a delivery acknowledgement. A missing job reply still
requires inspection; already-accepted work is not rolled back or replayed.
Validation: 207 workspace/all-target Rust tests passed with and without dashboard
support. The queue-saturation regression failed before the fix; it now verifies
prompt rejection, no late delivery, recovery after capacity frees, and separate
offline handling. The full 12-test gateway group passed 20 consecutive standalone
runs (240 test executions). Additional admission tests cover corrupt retained
history and 20 rounds of receipt creation racing shutdown, without orphan jobs.

Rebuilt headless processes passed node commands (`node-commands-PtQaP9`), job
shutdown/recovery (`job-shutdown-iAr9JH`), model/job controls including receipt
reuse (`model-metadata-C7NolE`) and two-logical-node scoped discovery
(`peer-discovery-iKmjK8`). These use isolated local fixtures and fake inference,
not a physical two-PC run. Formatting, JavaScript syntax, diff and backend/browser
dependency checks passed. No installed-node, model, permission or release-feed
change; no installer/signature or stable-0.4.0 readiness claim.

## Unreleased: gateway session shutdown ownership (2026-10-06)

New protocol regressions reproduced Stop returning with an authenticated session's
identity lease still reserved, and a probed connection still waiting for auth.
The listener previously spawned detached handlers and each handler spawned a
detached socket writer. Clearing the connection map did not retire those tasks.

The gateway now owns a reaped session task set. Stop signals its listener, drains
handlers including unfinished authentication, releases their socket halves and
identity leases, publishes disconnects for remaining live records and fails
pending calls before allowing a new generation. Read and write futures share the
handler lifetime. An interrupted Stop retains its handle/cleanup state so another
Stop can finish it; Start rejects that incomplete shutdown instead of overlapping
generations. This stops local transport, not already-dispatched remote effects.

Validation: 204 Rust tests passed in both feature modes at the gateway checkpoint;
later admission race/corruption tests raised this to 206. Protocol regressions
cover same-ID reconnection, pending replies, incomplete authentication and
cancelled Stop cleanup. The two lifecycle regressions failed before the fix.
Rebuilt headless CLI (`node-commands-0BNwrq`), shutdown/recovery
(`job-shutdown-P0g8tj`), model/job controls (`model-metadata-WBv0aW`) and two-logical-
node discovery (`peer-discovery-Wjc9Bq`) passed. All are isolated local fixtures,
not physical two-PC validation. No installed node or release artifact changed.

## Unreleased: retained-history creation deduplication (2026-10-06)

Node-origin `jobs.create` now atomically checks the authenticated origin/receipt
against retained journal records before insertion. Identical normalized input
returns the original job snapshot and `admission.reused:true`, without another
runner, history mutation or implicit resume. Changed input and ambiguous legacy
receipts fail closed. Original area context is retained across node moves; a
different origin cannot collide with or read another node's receipt.

The existing journal and admission gate are reused, with no second job framework,
schema migration, permanent tombstone collection or automatic retry. Protection
survives coordinator restart only while history remains. Removing that history,
switching databases or using older coordinators ends the guarantee. Owner-local
creation and non-creation controls retain their existing semantics. This is not
an execution grant, a remote stop acknowledgement or exactly-once execution.

Validation: 201 Rust tests passed in both dashboard/default and headless feature
configurations at this checkpoint. Tests cover 32 simultaneous identical creates,
one executor call, changed-input rejection, distinct owners, immutable origin area,
reopen with uncertain work and no implicit dispatch, ambiguous legacy receipts,
quota exhaustion, history removal and shutdown. Isolated headless protocol tests
passed (`model-metadata-N0nlSE`), including concurrent repeated receipts while
inference is held and reuse after completion/cancellation. The backend is a fake
Ollama HTTP fixture, not real inference or a physical two-PC test. Shutdown/recovery
also passed (`job-shutdown-dpq8CY`). Installed nodes and release feeds are unchanged.

## Unreleased: connection-stage diagnostics (2026-10-06)

The coordinator's existing CLI/API diagnostic report now distinguishes
authenticated connections, gateway-accepted registrations and registrations
still pending. The three counts use one live connection snapshot, never the
historical route map. Remote provider and healthy-provider counts are taken
separately from the scheduler registry, excluding local adapters. No node names,
addresses, paths, capabilities, permissions or model details are added to reports.

The report explicitly warns that these separate snapshots are not an execution
readiness guarantee. Registration precedes scheduler processing; a registered
node can advertise no capabilities/models; health does not reserve capacity or
check permissions. This makes the reconnect race found on September 27 visible
without changing job admission, adding retries or requiring dashboard code.

Validation: 197 Rust workspace/all-target tests passed both with and without
dashboard support. New regressions cover the encrypted authentication/register/
disconnect lifecycle, invalid and repeated registration, empty capability
advertisements, historical-route exclusion, gateway stop and remote-vs-local
provider counts (including unhealthy and at-capacity providers).

Rebuilt headless binaries passed command/API integration with no nodes and with
a model-free registered node (`node-commands-tPLxCI`), including redaction checks.
The shutdown/recovery regression also passed (`job-shutdown-2Qi0hd`). Formatting,
JavaScript syntax, diff and backend/browser dependency checks passed. These are
isolated local-process checks, not physical two-PC validation. This is unreleased
source work, with no installed-node, permission or update-feed change.

## Unreleased: Stop and runner-retirement coordination (2026-09-27)

A concurrent regression reproduced a Stop request leaving a job in `stopping`
after its last runner exited. Stop previously sampled runner presence separately
from saving its state, while runner cleanup checkpointed interruption separately
from removing that runner. Either gap could leave nobody to settle the request.

Stop's presence check/checkpoint and runner cleanup/removal now use the existing
admission gate, consistently locking runner state before the journal. No remote
call or await is performed under this gate. Stop before execution starts still
prevents dispatch; already-claimed work may finish, with its results retained and
dependent steps cancelled. An uncertain runner exit remains `interrupted` for
review, not a promise of confirmed remote cancellation. Repeating Stop on finished
history remains an error without rewriting the saved result.

This builds on the unreleased shutdown-admission fix below. It adds no execution
lease, remote cancellation, schema/wire change or media feature. No installed
node, release, permission, model or update feed is changed.

Validation: 194 Rust workspace/all-target tests passed with and without dashboard
support. The new race regression failed before the fix and then passed 20
consecutive standalone runs (512 raced retirements per run). Additional tests
verify Stop before a resumed runner's first poll, retention of a finished claimed
step, cancellation of its dependents, and unchanged history after a rejected
second Stop. Formatting and backend/browser boundary checks passed.

The first local-process shutdown run (`job-shutdown-zBtmmz`) exposed a separate
fixture readiness gap: its new job failed with no HTTP provider available after
socket reconnection. Authentication is counted before registration reaches the
scheduler. The harness now waits for the exact healthy HTTP provider before
submitting new work, records readiness, and saves terminal failures immediately
instead of waiting for a success-only timeout. It never retries a failed job.
Three corrected runs passed (`job-shutdown-aszVTz`, `job-shutdown-zFa6UY`,
`job-shutdown-6p7EqW`); each observed socket reconnection before provider readiness.

Rebuilt headless node commands (`node-commands-Dw037G`) and model/job controls
(`model-metadata-B2xIky`) passed. Model tests use a fake Ollama backend, not real
inference. JavaScript syntax, diff and documentation-link checks passed. No new
installer, physical two-PC deployment or remote-stop guarantee is claimed.

## Unreleased: shutdown-safe job admission (2026-09-26)

Backend development resumed after the documentation-only pause. A regression
against the unchanged alpha.6 engine reproduced a rejected enqueue leaving a
new pending job in the journal: persistence happened before the shutdown check.
New-job persistence, resume checkpointing and runner registration now share a
single admission/closing gate. Shutdown closes that gate before notifying and
draining runners, including jobs not yet polled. Failed admission creates no
runner guard, and missing-executor rejection saves no new job on either path.

This is in-process coordination, not durable command deduplication, remote
execution leases, remote cancellation or exactly-once effects. No schema/wire
change, model download, installed-node change or release is included. The prior
presence/media design remains planned; no UI feature work was resumed.

Validation: 191 Rust workspace/all-target tests passed with and without dashboard
support. Six new cases cover rejected enqueue/run admission, missing executors,
unchanged rejected-resume checkpoints, shutdown before first poll, concurrent
submission/shutdown, and failed quota admission followed by a successful retry.
The concurrent test also passed 30 consecutive standalone runs. The original
rejected-enqueue regression failed against the unchanged engine before the fix.

Rebuilt headless binaries passed local-process shutdown/recovery
(`job-shutdown-vXIWhf`), node commands (`node-commands-frrGFw`), and model/job
protocol regression (`model-metadata-MzTiFh`). The latter uses a fake Ollama
backend, not real model inference. Formatting, diff, documentation-link and
backend/browser boundary checks passed. No physical two-PC deployment or new
installer validation is claimed for this unreleased increment.

## 2026-09-22 documentation-only direction: presence and media

Recorded the owner's [presence and supervised media requirements](PRESENCE-AND-MEDIA.md):
fixed virtual audio input/output, user-preferred microphone/TTS mixing and output
routing, optional assistant participation, reusable lightweight presence for
welcoming/locating/handoff, and EEF-approved peer or coordinator-relayed sessions.
The design includes concurrent resource use, bounded bandwidth, local sensing,
authorization expiry and explicit privacy/automation preferences.

All remain planned. Development was paused at that checkpoint; no runtime code,
installed node, configuration, permission, release, model or update feed changes
accompanied that documentation. Alpha.6's actual validation remains recorded below.

## v0.4.0a6 release checkpoint

Alpha.6 packages the shutdown-order and command-response fixes documented below.
Those checkpoints describe their original pre-release validation, not alpha.6
availability. See [release notes](RELEASE-0.4.0a6.md) for final artifact and live
deployment evidence. Automatic-update feeds remain paused; no self-development
agent, permission expansion or model installation is introduced.

The alpha.6 optimized regressions and both installer validation suites passed.
The local coordinator is upgraded with configuration, database and startup choice
preserved. The physical two-PC in-flight interruption test now passes against an
existing alpha.5 node: interrupted checkpoint, unsafe-resume refusal, no observed
replay, and no late-response state change. The historical pre-release notes below
are retained as the validation sequence, not the current deployment state.

The release is published as a prerelease with two hash-verified installer assets.
The physical node was then updated from alpha.5 to alpha.6 through one exact-
artifact CLI request and a separate approved restart. Its staged binary passed
an on-node Defender scan, stable identity/configuration/permissions were preserved,
and all five version-matched probes passed. Both live automatic feeds remain
paused. This validates a successful explicit update, not crash-safe update
receipts, rollback or model workloads.
The physical interruption and durable-job persistence regressions also passed
again with both roles on alpha.6; test-owned history was saved and then removed.

## Post-alpha.5 interrupted-job shutdown ordering

Live two-PC tests on installed alpha.5 verified successful and failed remote
read jobs, bounded safe retries, subsequent healthy work, and persistence of
full stored job results across coordinator restart without redispatch.

A separate in-flight process test exposed a shutdown race: the gateway closed
before the job engine checkpointed interruption, so its synthetic "node gateway
stopped" response could mark an uncertain remote action as failed. The action
was not retried, but its state was misleading. Shutdown now interrupts and
drains job runners before stopping the gateway. This does not cancel remote
effects or establish exactly-once execution.

`tools/test-job-shutdown.mjs` reproduces the problem with the published alpha.5
binaries using an isolated held HTTP request, without shell commands or personal
settings. The rebuilt headless source passes interruption, unsafe-resume refusal,
no replay, late-response isolation and subsequent healthy-work checks. The
installed alpha.5 applications and published installers are unchanged; the fix
requires a future build. Private live-test records remain outside the repository.

Validation: 185 Rust tests passed with and without dashboard support. The new
process regression failed against the published alpha.5 binaries and passed
four consecutive times against rebuilt headless binaries. Existing node-command
and explicit-update suites, formatting and backend/browser boundary checks passed.
Live test histories were removed only after their evidence was saved; the real
network remains on alpha.5 with no active test jobs or changed permissions.

## Post-alpha.5 command compatibility diagnostics

The physical update test found an empty HTTP 404 when a new restart CLI contacts
an older coordinator without that endpoint. The CLI now explains the possible
coordinator-version mismatch instead of exposing a JSON EOF error. Malformed
responses include HTTP status and unconfirmed-outcome guidance. No fallback,
retry, permission change or new command endpoint is introduced. This source
change is not in the already-published alpha.5 installers.

Validation: 185 Rust tests passed with and without dashboard support, plus
formatting and backend/browser boundary checks. A rebuilt headless CLI issued a
read-only model-list request to the real older coordinator and reported the
actionable HTTP 404 error with exit status 1, without fallback or retry.

## v0.4.0a5 release checkpoint

Alpha.5 includes the explicit-update correction below and a paused-feed response
that contains no artifact. Both public feeds are withdrawn while all releases
are prereleases. New clients report `feed_paused`; older clients fail closed.
See [release notes](RELEASE-0.4.0a5.md) for build and deployment evidence.
The following explicit-update checkpoint describes source validation before
alpha.5 packaging; its historical "unreleased" label is no longer its availability.

## Unreleased explicit remote update correction

- `eef node update` selects an exact version/HTTPS artifact/hash/size, with
  per-request prerelease consent. The distinct authenticated node action honors
  remote-update approval, checks current owner revocation again before activation,
  preserves stable-only automatic feeds and does not need process/filesystem grants.
- A shared installation lock prevents concurrent explicit/feed installs; newer
  pending selections are preserved. Bundle product/version and artifact integrity
  are verified. `eef node update-status` reads the actual installed selector.
  Restart remains a separate approved command. No command or automatic fallback
  can silently opt old nodes into prereleases.
- The fix is not inside published alpha.4. No published installer, installed
  node, stable manifest or teacher-submission repository is changed by this source
  increment. See [command and recovery limits](COMMANDS.md#explicit-remote-updates).
- Validation checkpoint 2026-09-20: all 183 Rust tests passed with and without
  dashboard support. Tests cover strict one-shot consent, URL/hash/size validation,
  downgrade/pending selection refusal, installation locking, corrupt selectors,
  artifact integrity, product/version matching, activation-time revocation and
  unchanged configuration. Stable-feed tests still reject prereleases even when
  a manifest includes opt-in fields. Payload fixtures are never executed.
- `explicit-updates-fx10vh` passed real local command/transport denial and
  permission revocation with headless binaries; mock coordinator replies cover
  exact confirmation, lost replies, older nodes and no replay/fallback. No real
  release download or installation was performed by this process suite.
  `node-commands-YnBFtD`, formatting, JavaScript syntax and backend/browser
  dependency checks passed. An earlier dashboard-enabled command run also passed
  (`explicit-updates-B2o0VA`).

## v0.4.0a4 release checkpoint

The following development checkpoints through owner-selected request preview
are included in v0.4.0a4. Their historical "unreleased" labels describe their
original validation dates, not alpha.4 availability. See the
[release notes](RELEASE-0.4.0a4.md) for packaging and deployment evidence.
No default interpreter is approved and preview never dispatches work.

## Unreleased owner-selected request preview

- `eefn requests preview --text TEXT --json` now uses exactly one applied model
  with `llm.infer` and the explicit `request_interpreter` role. It uses the shared
  strict proposal contract, retains original input, requires reported completion,
  and always reports no dispatch/no execution authority. No job, permission,
  default model, download or coordinator submission is created.
- The service has one-preview capacity, a 20-second deadline and a 512-token
  output limit. A weak runtime binding is invalidated on node restart so old
  results cannot become current proposals. Missing/ambiguous roles, invalid or
  partial results and unavailable backends fail without fallback. CLI discovery,
  target/correlation checks, origin guard and no-replay behavior work without UI
  or an EEF connection. External providers may continue compute after disconnect.
- Validation checkpoint 2026-09-20: 178 Rust tests passed with and without
  dashboard support. `request-preview-nFGNN7` passed paused-node CLI preview,
  original text retention, invalid/incomplete rejection, role checks, exact target,
  origin guard, busy admission, timeout/capacity release, restart invalidation,
  lost-reply/no-retry and old-endpoint/no-fallback checks. `model-startup-8ieqHk`
  additionally passed llama.cpp preview and model lifecycle regressions.
  `model-metadata-Qv24TM` and `model-downloads-Fy8QLw` passed existing model/job,
  response-boundary, approval and transfer regressions. All process backends were
  synthetic, not a real interpreter quality or physical two-PC acceptance claim.
- Automatic chat interpretation, interactive clarification/approval, EEF context
  validation and execution grants remain planned. No release, installed node or
  teacher-submission snapshot changed. See [usage](COMMANDS.md#request-preview-development)
  and [contract limits](INTERPRETATION.md).

## Unreleased bounded inference response handling

- Ollama and llama.cpp now share bounded chat normalization: 4 MiB backend JSON
  limit for declared and chunked bodies, rejection of missing/non-text completions
  and provider errors, plus additive `finish_reason` / `completion_complete`
  metadata. Missing or unknown completion evidence stays unknown. A valid result
  never grants tool execution and requests are not automatically retried.
- Validation on 2026-09-19: 176 Rust tests passed with and without dashboard
  support. `model-startup-uO7sIk` checked completed/partial/missing llama.cpp fixture
  responses over the real node connection along with startup/crash regressions.
  `model-metadata-sn1HSi` checked complete/partial/unknown Ollama fixture responses,
  missing content, declared/chunked overflow, stalled-body timeout with no replay,
  and existing model/job/approval behavior. `model-downloads-tW5Y1e` passed transfer
  regressions. Source boundary, formatting and diff checks passed.
- Fake backend acceptance only for this increment. No interpreter was enabled,
  no additional model weights were fetched, and no installed node or release was
  changed. See [response contract](NODE_MODELS.md#inference-response-bounds-development).

## Unreleased interpretation contract foundation

- Added a shared typed Rust proposal parser/prompt. Original input is preserved;
  version/type/byte/collection limits, mandatory nullable fields, duplicates,
  unknown fields, capability context and clarification consistency are checked.
  Every accepted proposal remains untrusted and requires review or clarification;
  it cannot provide execution authority, origin identity or target/grant fields.
- This is not integrated into chat, command submission, EEF planning or live
  interpretation. No model role/default or automatic actions were enabled. The
  test-only stdin example is not packaged; the CPU harness's `contract` profile
  uses that native parser rather than duplicating validation in JavaScript.
- Validation on 2026-09-19: 173 Rust tests passed with and without dashboard
  support. Process suites passed in `model-startup-BRHv2I`,
  `model-downloads-YMYajE` and `model-metadata-ITt7MW`. Real CPU contract evaluation
  in `model-benchmark-gmKt3L` completed: 6/12 outputs were structurally accepted,
  0/12 matched both expected intent and clarification. All accepted reports
  retained original text and `execution_authorized: false`. The candidate remains
  unapproved; schema validation is not semantic accuracy or a safety proof.
- See [contract and remaining integration](INTERPRETATION.md) and
  [candidate evaluation](MODEL-CANDIDATE-EVALUATION.md). No additional weights
  were downloaded for this increment; installed nodes and teacher snapshot stayed
  unchanged.

## Unreleased lightweight-model CPU evaluation

- Added an opt-in bounded CPU benchmark that verifies an existing catalog
  artifact and runs fixed synthetic classification inputs without executing
  actions or changing node configuration. It records runtime identity, startup,
  Windows peak working set, latency and strict output-schema/expected-intent counts.
- On 2026-09-19 the pinned 491,400,032-byte candidate was downloaded once into
  ignored `.validation/model-artifacts/` and its SHA-256 verified. Real CPU runs
  completed in `model-benchmark-MYTDGw` and `model-benchmark-5uqQZo`: baseline
  4/12 expected classifications, detailed prompt 9/12. No default was approved.
  See [method, evidence, upstream license references and limits](MODEL-CANDIDATE-EVALUATION.md).
- This is one-PC runtime feasibility, not physical two-PC acceptance or a
  security evaluation. Installed nodes, stable update feeds and teacher snapshot
  remain unchanged. The reusable development model cache uses about 469 MiB.

## Unreleased post-startup model process monitoring

- A runtime-scoped monitor checks the owned GGUF group once per second after
  readiness. An observed exit, missing owned process or process-status error
  withdraws group availability and stops remaining owned model children. Node
  commands remain available; saved selections are retained and retry requires
  explicit restart. No work is replayed and no automatic respawn is performed.
- Existing authenticated registration refresh propagates the error without an
  intentional reconnect. Local model/capability status also updates with the
  connection paused or unreachable. Runtime exits are not startup issue codes.
  Detection and propagation are not instantaneous; ongoing HTTP health, hangs,
  successful inference, per-slot recovery and resource usage remain unmeasured.
- Validation on 2026-09-19: 167 Rust workspace/all-target tests passed with and
  without dashboard support. `.validation/model-startup-c2Vj4K` killed a ready
  fixture child and verified unavailable/error metadata, surviving-sibling
  cleanup, command availability, unchanged connection/runtime identity, explicit
  restart recovery and paused-connection failure reporting. Existing loading
  and restart-cleanup checks also passed. `.validation/model-downloads-ltpT2Z`
  and `.validation/model-metadata-QbhXFk` passed transfer/receipt/approval and
  model/job regressions. The backend/browser boundary check and formatting passed.
- These are isolated fake-backend process tests, not real inference or physical
  two-PC acceptance. No model weights, release, installed node, stable update
  policy or teacher-submission snapshot was changed.

## Unreleased model install preflight

- `eefn models install-plan --model ID` inspects the provider, source/license
  metadata, versioned GGUF artifact contract, local disk snapshot, cache candidate
  and active-download state without fetching weights or changing node state.
- Installation uses the same artifact validator before admitting a transfer.
  Invalid/ambiguous catalog entries and known disk failures leave prior receipts
  untouched. Cache reuse checks exact size as well as digest. Existing remote
  approval/configuration admission checks and transfer-time limits remain in force.
- Explicit llama.cpp selection no longer contacts Ollama to decide its backend.
  Ollama plans do not mislabel GGUF size/hash as provider metadata. License review,
  runtime compatibility and default-model/bootstrap policy remain unverified or
  planned; no model artifact, installed node or release was changed.
- Validation on 2026-09-13: 165 Rust tests passed with and without dashboard support,
  including unsafe/incomplete artifact metadata, insufficient/unknown space,
  cache size/digest semantics, duplicate catalog IDs and preservation of prior
  receipts after failed admission. Isolated `model-downloads-LaQNkt` verified
  read-only CLI preflight, provider-specific unknowns, active-transfer blocking,
  no Ollama request for explicit GGUF, and rejection after a catalog change.
  `model-metadata-7fbTQb` and `model-startup-0rIH0b` passed the existing model/job,
  approval, receipt and startup regressions. Evidence is under `.validation/`;
  fake backends only, no real inference or physical multi-node acceptance.

## Unreleased command-based model installation

- `eefn models installed`, `inspect`, `install`, `download-status` and
  `cancel-download` use a typed local command service backed by the existing model
  installer. They need a running node but no browser, EEF connection or model.
- CLI install attempts carry a pre-send UUID. Lost replies remain unknown and
  are never automatically replayed. Exact-ID status/cancellation cannot target
  a newer transfer; the currently retained ID cannot be admitted twice.
  Only the latest in-memory transfer is retained, not a durable receipt ledger.
- Selection commands remain compatible. Installation does not select/load a
  model, enable permissions or restart the node. No default model, bootstrap
  policy, UI changes, release or physical-node deployment is included here.
- Validation on 2026-09-13: 161 Rust tests passed with and without dashboard support.
  Isolated `.validation/model-downloads-lkTc15` passed installed-model inspection,
  explicit admission, progress after CLI exit, exact-ID cancellation, one lost
  reply recovered by receipt, refusal of duplicate/stale IDs and no legacy API
  fallback. `.validation/model-metadata-Cq58tM` and `model-startup-vW9fZA` passed
  the existing model/job/approval and nonblocking-startup regressions. Fake local
  backends only, no real model downloads/inference or physical two-PC claim.
  Validation disabled debug symbols and incremental compilation to limit disk
  growth; no installed application, default update feed or global Git identity
  was changed.

## Unreleased nonblocking local model startup

- GGUF group startup no longer holds up node registration, heartbeats, jobs or
  restart commands. A scoped loader is cancelled with the runtime and cleans up
  partially started children. Startup failure retains owner selections.
- Loading/error/unloaded groups remain visible with unavailable metadata; both
  capability advertisement and direct inference refuse non-ready groups.
  Readiness refreshes the existing authenticated registration without reconnect.
  EEF preserves unchanged capability in-flight counts, limits and measured load
  atomically while replacing the advertised capability snapshot.
- No default model, interpreter, inference weights, peer transport, UI changes,
  installer changes or physical-node deployment are included in this increment.
  Group readiness is not ongoing health monitoring or per-slot lifecycle control.
- Validation on 2026-09-13: 157 Rust workspace tests passed, including the
  dashboard-free build and duplicate-capability refresh/accounting regression.
  The initial default-feature pass also passed all 157 tests. Final API-only
  process fixtures passed in `.validation/model-startup-Rt9wAe` and
  `.validation/model-metadata-2tkVvQ`: registration/commands while health is gated,
  restart cleanup of loading and ready children, readiness refresh without a
  reconnect, paused-connection loading/readiness, invalid/missing optional runtime
  isolation, job/receipt recovery and approval checks. Fake backends only, not
  physical two-PC inference. Source boundary, formatting and diff checks passed.

## v0.4.0a3 release preparation

This opt-in alpha packages the backend checkpoints below. Stable update feeds
remain unchanged. Both roles now reject prerelease feed updates at check and
installation time; a changed manifest cannot bypass the installation guard.
See [release scope and limits](RELEASE-0.4.0a3.md).

## Unreleased optional runtime startup isolation

- Failed llama.cpp startup now cleans up the local model group and continues
  node registration without advertising that group. Missing Python or failed
  plugin inspection withholds affected capabilities rather than disabling the
  core connection. Owner settings remain unchanged; no silent model substitution.
- Status and diagnostics expose bounded `startup_issues` codes, without raw
  exception text or paths. This is a startup snapshot, not continuous health.
  Nonblocking model loading and individual slot recovery remain future work.
- All 153 Rust tests passed both with and without dashboard features on
  2026-09-13. Isolated `.validation/model-metadata-rIcvW6` verified connected job
  commands and system ping despite missing model/Python runtimes, plus approved
  remote restart and recovery. It also passed receipt-loss and authorization
  revocation checks. No physical model/hardware test or deployment is implied.
- Command/discovery, first-run and browser-copy regressions passed:
  `.validation/node-commands-oRA9UM`, `.validation/peer-discovery-UfZBLg`,
  `.validation/first-run-Zbjo2h`. No model download or installed-node change.

## Unreleased job receipt recovery

- The node CLI generates an operation receipt before sending. Updated EEF stores
  it in the authenticated job origin context while keeping transport reply IDs
  independent. `eefn jobs find --operation-id UUID` searches only that origin's
  retained creation history, filtering before the bounded result page.
- Lookup does not replay work or deduplicate submissions; an empty match is not
  proof that an uncertain request was never accepted. It is not a control-command
  receipt ledger. See [command recovery semantics](COMMANDS.md).
- All 152 Rust tests passed on 2026-09-13, including persistence/reopen, filtering
  beyond the first 100 jobs, origin scoping and independently correlated replies.
  Isolated `.validation/model-metadata-bnagjD` deliberately dropped the local
  creation reply, verified exactly one submission and recovered the saved job
  using the CLI receipt. It also passed the earlier model/auth/job/restart checks.
- A read-only installed-coordinator check still showed two connected nodes,
  two registered models and no pending restart. Those v0.4.0a2 installations were
  not updated; new-code execution used a fake backend, not physical inference.

## Unreleased remote authorization race fixes

- Legacy remote configuration writes now read, authorize and save under the
  same lock as local commands. They preserve concurrent owner changes and
  cannot restore revoked management approval. Denied writes remain proposals.
- Remote model installs recheck approval at download admission after backend
  inspection, and reject changed model configuration. Approval revocation does
  not retroactively cancel already admitted work; explicit cancellation remains
  available. No new model source or automatic download was added.
- All 150 Rust tests passed on 2026-09-13. The isolated process regression
  `.validation/model-metadata-E9N5Nq` revoked permission during fake backend
  discovery and verified that no pull started, then checked denied remote saves.
  No model weights, installed apps, physical node settings or releases changed.

## Unreleased backend/browser separation checkpoint

- Browser assets are optional in both crates; both executables support `--no-ui`.
  Node HTTP routes moved to `api.rs`, with core status/configuration, connection,
  proposal, startup and update operations owned by `NodeService`. Browser files
  contain presentation routes/native picker only. A source dependency guard
  checks that runtime modules do not import the browser/HTTP adapters.
- Added `eefn connection show/pause/resume/pair-local`. Pausing the outgoing EEF
  connection does not disable the local command API. Connection config edits
  serialize with model edits, preserving unrelated saved settings.
- Saved `dashboard.ui_enabled` / `web.ui_enabled` preferences hide just the
  browser, without changing legacy listener enablement. The node adapter can
  change its browser gate without rebinding its command listener. CLI `--no-ui`
  remains an override; no-dashboard builds cannot enable browser assets.
- 148 default-feature Rust tests passed on 2026-09-12, including browser-gate
  and configuration validation. Saved-preference integration passed in
  `.validation/model-metadata-S3J9GL`: both roles' commands worked with UI hidden,
  node UI enable/disable preserved the command endpoint, and connection controls,
  models, jobs and restarts passed against an isolated fake HTTP backend.
- The earlier no-dashboard build passed 146 Rust tests and process integration
  (`.validation/model-metadata-2i2Vce`); all 148 tests also passed without dashboard
  features on 2026-09-13 after the saved-preference follow-up. This is not real inference or physical
  two-PC acceptance. No installed app, model, release feed or signing status was
  changed. See [boundary and remaining scope](BACKEND-BOUNDARY.md).
- Follow-up command/discovery, first-run/job and browser-copy regressions passed
  on 2026-09-13 (`.validation/node-commands-6qd4MR`,
  `.validation/peer-discovery-yOxtQ3`, `.validation/first-run-RPsvjj`).

## Unreleased node-origin job commands

- Added `eefn jobs list/get/output/pause/resume/stop/remove/generate-text` and
  guarded typed `/api/commands/jobs`. The runtime service uses the running node's
  existing submission connection and actual API discovery; no second node,
  browser controller, local LM or remote-management permission is required.
- EEF preserves authenticated origin ownership and existing durable lifecycle
  rules. Creation acknowledges a job, not completed work; pause/stop report
  intermediate states. Commands distinguish known-not-sent from rejected or
  uncertain mutations and never automatically resubmit after a lost reply.
- Explicit output inspection returns bounded text-model content only, while
  ordinary status remains redacted. It uses the same origin check as controls;
  it does not export file contents, media, parameters or another origin's results.
- 144 Rust tests passed on 2026-09-12. Isolated integration evidence:
  `.validation/model-metadata-YCa5L1` verified origin scoping, text output,
  pause/resume without repeated execution, stop and finished-history removal,
  alongside remote model selection and restart tests. The backend was a fake
  local HTTP service, not real inference or a physical two-PC test.
- Command and discovery regressions passed (`.validation/node-commands-9NIgvh`,
  `.validation/peer-discovery-ltgV03`), as did first-run/job persistence and
  browser-copy regressions (`.validation/first-run-cZdAMC`). No installed app, release artifact or
  model was changed or downloaded. UI development remains paused.

## Unreleased coordinator-to-node model commands

- EEF now exposes `node models` using the same typed model-selection operations
  and CLI adapter as EEFN. Mutations require node-local management approval,
  rechecked under the config lock. Inspection remains read-only. No implicit
  downloads, loads, restarts or legacy full-config save fallback.
- Requests are sent once; uncertain replies are explicitly unconfirmed. Remote
  GGUF/projector paths are validated on the target node, not the coordinator.
- 140 Rust tests passed on 2026-09-12. Isolated integration evidence:
  `.validation/model-metadata-FyzHyP`; old-node refusal/compatibility:
  `.validation/model-metadata-r6gwng`. Command, discovery, first-run/job and
  browser-copy regressions passed (`.validation/node-commands-gHb7T0`,
  `.validation/peer-discovery-lez654`, `.validation/first-run-6iYR9q`).
- A read-only check of the installed coordinator still reported two connected
  nodes and no pending restart. These installed v0.4.0a2 apps were not changed;
  the new feature tests used isolated processes and a fake backend, not real
  inference. No release or automatic telemetry upload was performed.

## Unreleased explicit model-role routing checkpoint

- Added bounded, read-only `eef models route` and its guarded API. It previews
  connected candidates by capability/role/node/backend/model without inference,
  permission changes, readiness claims or reservations.
- Existing routing enforces explicit `model_role` and backend constraints even
  during tier fallback. Existing durable jobs support `generate_text` and
  `analyze_image`; automatic interpretation/planning remains pending.
- 135 Rust tests passed on 2026-09-12. Isolated integration evidence:
  `.validation/model-metadata-y8lWNI`. A fake backend verified exact role dispatch,
  missing-role job failure without fallback, and no inference during preview.
  This is not real inference or a physical two-PC test. Installed apps and
  published releases remain unchanged.

## Unreleased local restart and API discovery checkpoint

- Added `eefn restart` with local-owner node/runtime preconditions, one send,
  bounded completion observation and explicit uncertain outcomes. It shares the
  node core restart operation with existing browser/approved remote controls.
  Completion is a new initialized runtime, not a workload/model/reconnection claim.
- A bounded per-config local API marker records the actual bound endpoint.
  Network/model/input commands and restart polling follow it instead of pending
  port settings. It is identity-checked and loopback-only, with old-node fallback
  only when the marker is absent. No remote listener was added.
- 133 Rust tests passed on 2026-09-12. Isolated process testing verified restart
  and API port migration (`.validation/model-metadata-YyzqX8`); command and
  discovery tests passed (`.validation/node-commands-g9t1Vl`,
  `.validation/peer-discovery-QXwoD4`), as did first-run/job/browser regressions
  (`.validation/first-run-ezjgWB`). No real model download or release deployment.

## Unreleased node model-selection commands

- Added shared node `models show/select-ollama/select-gguf/hints/provider/remove`
  commands and the guarded local owner API. Offline commands use the instance lock;
  running commands use the existing node service and configuration lock. No JSON
  editing, model download or automatic restart is required to save a selection.
- Extended existing backend selections with optional capability restrictions and
  role labels, projected into metadata and enforced by updated node executors.
  Unsupported capabilities are rejected; an empty capability list disables that
  model's inference operations. Role-based scheduling is still pending.
- Saved selections and registered models remain distinct. No-op edits avoid writes;
  corrupt/missing config, wrong targets and invalid hints are rejected. Removal
  affects selection only, not model files. GGUF ports can be allocated internally.
- 130 Rust tests passed on 2026-09-12. Evidence:
  `.validation/model-metadata-3wV2vO`, `.validation/node-commands-YJoF6P`,
  `.validation/first-run-lVU6xZ`, `.validation/peer-discovery-sDhh29`;
  browser copying/focus checks also passed. Tests use isolated configs and a fake
  backend, not real inference. Published apps/feeds and the real network were not
  changed. See [commands and downgrade caveat](COMMANDS.md).

## Unreleased versioned model metadata checkpoint

- Added a shared, bounded `model_metadata` registration schema: multiple
  capabilities/input/output modalities, optional roles, lifecycle, availability
  and resource estimates. New Ollama/GGUF advertisements preserve legacy fields.
  Unknown values remain unknown; model names never infer capabilities.
- Registration validates the entire model snapshot before replacing it, including
  empty snapshots. Other nodes and load metrics are preserved. Duplicate identities
  and malformed/unsupported metadata are rejected, not silently downgraded.
- Inventory and scoped peer discovery carry validated metadata. Existing text/vision
  routing filters declared capabilities, input/output support and unavailable/error/
  loading state. Backend-specific failures no longer poison another backend's model;
  updated nodes enforce an explicitly requested backend without silent fallback.
- General owner configuration for non-text/vision models, role-based planning,
  default LM, resource-aware admission, lifecycle controllers and leases remain
  pending. See [contract and compatibility](MODEL-METADATA.md). No UI changes.
- 124 Rust tests passed. Isolated fake-backend protocol tests passed with both
  development binaries (`.validation/model-metadata-wnlYlL`) and the published older
  node (`.validation/model-metadata-G5SVAH`), using separate temporary configs.
  They verify registration, CLI/inventory, stable identity, model-list clearing,
  and new-node backend enforcement. These are not real inference or two-PC tests.
- Command integration (`.validation/node-commands-uOMOCv`) and scoped discovery
  (`.validation/peer-discovery-rxQzSI`), first-run/job persistence
  (`.validation/first-run-OQ15vZ`), and dashboard copying/focus regressions passed.
  Validation recorded on 2026-09-12. Published
  installers/feeds and both real installed apps remain unchanged; no model was
  downloaded. No signing or Microsoft clearance is claimed for this checkpoint.
  Both real nodes remained connected; installed executable hashes were unchanged.

## Unreleased model-inventory and approved restart checkpoint

- After the owner enabled local remote management, the physical second-PC node
  acknowledged restart and reconnected under the same stable ID with a new runtime
  ID. Evidence: `.validation/live-discovery-QGI8x0/results.json`. This closes the
  positive remote-restart gate left pending in the earlier checkpoint below.
- Added `eef models list` and guarded GET `/api/commands/models`: bounded pages of
  registered models, node/capability filters, distinct node/backend/model identity,
  and registration freshness. Unknown roles, lifecycle and resource estimates are
  null; paths, addresses and secrets are omitted. No backend scan/load/download.
- This is a read-only legacy normalization slice. General multi-capability model
  configuration and registration, role selection, and scheduling changes remain
  pending; published v0.4.0a2 nodes remain compatible. No visual UI work.
- 113 Rust tests passed, including model normalization, redaction, empty inventory,
  identity conflicts, paging and response limits. Command, peer-discovery,
  first-run and browser copying/focus regression checks passed.

Final inventory checkpoint evidence (2026-09-12 local time):
`.validation/node-commands-SzT9ir`, `.validation/peer-discovery-vsS9dj`,
`.validation/first-run-VTll5F`, and
`.validation/live-discovery-3S9iBX/results.json`. The live run used two physical
PCs and published v0.4.0a2 nodes. Inventory reported two advertised models on the
local node and an empty model list on the connected second-PC node. Discovery
grant/revoke, two coordinator restarts, and approved remote node restart passed;
all ten remote pings passed at 2.82-6.14 ms. No inference or model download ran.
The development coordinator SHA-256 was
`18a9a4189cc0711a5a60296448325e427ff398b9bc245baac500f96e9548aefc`.
Its local Defender scan passed with engine `1.1.26080.3`, definitions
`1.459.158.0`; this is not Microsoft clearance or a signing claim.
The original discovery policy and published coordinator executable were restored
(hash verified); both nodes reconnected with no pending coordinator restart.
No release assets, feeds or remote node binaries were changed.

## Unreleased command/restart follow-up

- Added coordinator-owner `discovery show/grant/revoke` and `restart` commands,
  backed by shared core services and guarded local APIs. Exact, directional
  disclosure only; saved/applied policy and restart completion remain distinct.
- Serialized saved-config edits preserve unrelated pending settings, reject
  corrupt/missing configuration and keep no-op grants from rewriting files.
- Added `eef node restart --node ID`, using the existing node-owner management
  approval. Bounded completion checks require the same stable node ID and a new
  runtime ID. Lost replies/timeouts are uncertain, not automatic restart retries.
- Bounded outbound request queue waits and cleaned up cancelled response waits.
  These changes are in the coordinator's shared transport; no new wire protocol.
- 109 Rust tests passed. Physical testing confirmed real two-node
  registration, grant/revoke behavior, reconnect after two coordinator restarts,
  and 10/10 remote pings. The remote node's restart permission was off: denial
  passed; positive physical remote restart was confirmed in the subsequent checkpoint above.
- Published v0.4.0a2 installers/feeds are unchanged. No UI redesign, permission
  widening, model downloads or signing claim. See [commands](COMMANDS.md).

Final follow-up evidence (2026-09-11): `.validation/peer-discovery-21turW`,
`.validation/node-commands-BUeKRY`, `.validation/first-run-LrJf2O`, and
`.validation/live-discovery-wIG53K/results.json`. The live run used two physical
PCs with published v0.4.0a2 nodes and a locally built development coordinator
(SHA-256 `8fadfbfd78a64be8e3f6d8658ed4b7a8aa9ac909abf4217919cb51abdd379eda`).
Its local Defender scan passed with engine `1.1.26080.3`, definitions
`1.459.158.0`; no Microsoft clearance is implied. The final ten remote ping
samples passed at 3.00-5.78 ms. Reconnect/grant/revoke and denied remote restart
passed; no actual second-PC node restart was sent without local approval.
After restoration, both nodes reconnected, no pending restart remained, the
original self-only discovery policy was applied, and the published executable's
hash matched its backup. No release assets or test network secrets were published.

## v0.4.0a2 diagnostic alpha

Workspace version `0.4.0-alpha.2` packages the command/service and scoped-discovery
increments below, plus minimal local diagnostics, authenticated ping/latency
probes and explicit-address private pairing codes. See
[release scope](RELEASE-0.4.0a2.md) and [two-PC testing](TWO_PC_TEST.md).
98 Rust tests and final packaged command/discovery/first-run/browser and installer
validation passed; hashes and scan details are in the release notes. Physical
connectivity was subsequently confirmed; full workload acceptance remains pending.
MSI, default LM, independent peer credentials and leases are not
claimed by this test release. Diagnostic reports are local, not automatic uploads.

## Active direction (2026-09-11)

[Command-first core roadmap](ROADMAP-CORE.md) supersedes the original milestone
order. UI polish is paused. Shared command services, identity/address separation,
coordinator advertisements, general model lists, a default lightweight node LM,
structured interpretation, authorized workloads and local/peer execution are
the next increments. Most are still planned extensions, not completed features.
The published v0.4.0a implementation below remains the baseline; its installers
still do not supply/download a model.

Implementation packaged in v0.4.0a2: shared `NodeService` state/model operations, network
show/set commands, API-only `--no-ui`, validated independent node/coordinator
advertisements and EEF node-view propagation. 91 Rust tests and command/browser
integration passed; see [commands and limitations](COMMANDS.md). Peer-address
distribution is now implemented as exact owner-scoped pull discovery with
monotonic freshness and bounded sanitized pages (`network peers`). Discovery
does not authorize direct communication. Leases, general model metadata,
default LM and MSI remain pending. The discovery follow-up passed 97 Rust tests
and a two-logical-node command integration test; physical acceptance is pending.

## v0.4.0a alpha scope

The earlier workspace was `0.4.0-alpha.1`, presented as **0.4.0a**. The owner approved
shipping this reduced alpha scope and deferring the remaining features.
See [RELEASE-0.4.0a.md](RELEASE-0.4.0a.md) and [ROADMAP-0.4.0.md](ROADMAP-0.4.0.md)
for the historical prerequisites and seven milestones. They are not all
implemented; development now covers the reliability prerequisite and an
origin/area/resource-routing, model management, and durable one-shot job
implementations with automated acceptance checks.

- Node-first browser and CLI input over the running node's authenticated
  connection; bounded pending requests, no submission replay after disconnect.
- Encrypted EEF reachability probe before authentication/registration, without
  creating a node record. Older EEF's encrypted auth denial is accepted only as
  service reachability; a fresh socket still requires normal authentication.
- Persistent Stop/Resume connection controls. Stop waiting for a reply is
  explicitly distinct from cancelling work already running on other nodes.
- Mutating/unknown tasks no longer retry automatically unless their caller
  explicitly sets `constraints.retry_safe`; read-only retry behavior remains.
- Feature-wide permission switches, retaining default denial and old scoped
  configurations. Remote update installation requires its own permission and
  cannot replace the locally selected update feed.
- Compact navy/blue dashboard, node terminology, stable DOM updates that preserve
  selection and editing, and decorative icons excluded from accessible names.
- Bounded inbound frame reads, random authentication nonces, atomic live identity
  reservation, and response matching against the authenticated executor.
- Update extraction checks unpacked size against free space, stages new files,
  and refuses to delete an already installed version. No fixed 5 GB reserve.
- Optional signing build hooks and certificate-aware payload framing. No public
  certificate has been configured and no signed release has been produced.
- Authenticated submission-origin snapshots retained through goals, tasks,
  inference transport, events, replies and bounded SQLite conversation history.
  Compatible metadata registration and additive history migration.
- Hierarchical node/resource areas, node-qualified stable resource IDs, owner
  adapter bindings and locality-aware routing after suitability/freshness checks.
  Explicit unavailable resources fail without falling back to another resource.
- Node location/resource forms, also available through EEF remote configuration.
  Resource creation is independent of permission grants and hardware activation.
- Remote-save now explicitly includes validated metadata; remote status returns
  applied areas/resources and last-request diagnostics. Tests cover both proposal
  approval and trusted remote management without changing identity or grants.
- Model selection inspects Ollama's reported capabilities on demand. Unknown
  capabilities require an explicit owner choice; embedding-only models are not
  offered as text generation. Selected vision models can also serve text requests.
- Model-service metadata is bounded to 4 MiB; progress lines to 64 KiB and 4096
  layers. Progress retains layer counts during verification. Cancellation has its
  own terminal state and optional job-ID matching; it is not an installation error.
- GGUF downloads recheck remaining disk space during transfer, verify existing
  files before reuse, and preserve damaged files/indexes instead of overwriting
  them. The UI distinguishes node-drive free space from unknown Ollama storage.
- Update manifests are capped at 1 MiB and artifacts at 512 MiB, with optional
  exact `size_bytes`, hexadecimal checksums, and streamed bounds even without
  Content-Length. Artifacts are still buffered in memory within that limit.

- SQLite-backed one-shot job definitions, attempts, results, origin and executor
  assignments. Checkpoints precede dispatch and dependent steps. A local database
  ownership lock prevents two coordinators opening the same job journal.
- Interrupted jobs do not execute on startup. Explicit resume retains completed
  steps and refuses uncertain non-idempotent actions. Pause/Stop finish the current
  claimed batch; they do not acknowledge remote cancellation or undo effects.
- Jobs history, step details and Pause/Resume/Stop controls in the node app over
  its existing authenticated connection. Nodes see/control only their own origin
  jobs; the loopback EEF owner API can manage all jobs. Finished history removal
  is explicit. UI responses omit raw parameters, file contents and media results.
- Shutdown joins node submissions and interrupts active runners before reopening
  the journal. Cancellation drops scheduler capacity guards. Runtime instance IDs
  let integration checks distinguish the old instance from a completed restart.
- State-dependent recovery headroom within the logical job budget keeps safe
  interruption/stop/removal available even after lowering the quota. Older
  development journals migrate additively, including partial migration recovery.
  Dashboard forms configure history count and MiB budget; Jobs shows effective
  network usage. Physical disk exhaustion and corruption still fail closed.
- EEF configuration forms load saved preferences, not stale running settings,
  while status keeps reporting applied settings until restart. Subsequent saves
  preserve a redacted saved network key rather than reverting to the running key.

Known remaining boundaries: real two-PC acceptance, remote-task cancellation
acknowledgements, verified Ollama-server storage reporting,
physical resource acceptance, recurring jobs and controlled migration,
media relay/correlation, Wi-Fi and
Bluetooth controls, mesh/ESP-NOW, distributed optimization, and coordinator
consensus/replication/fencing are not completed by this slice. Defender reports
against v0.3.2 remain unresolved; passing development tests do not clear them.

This file distinguishes working behavior from planned architecture. It prevents
the open-source project from presenting an unsafe approximation as complete.

## UX/UI changes in 0.3.2

- Graphical per-user installer with folder picker, startup/launch choices,
  extraction progress, and a completion screen. No administrator bypass.
- Home, Devices, Models, Permissions, Network, Settings, and Advanced pages;
  form-based common settings, explicit save/apply/restart, and mobile layout.
- Persistent generated device ID, detected hostname, single-instance launch,
  and same-account local EEF pairing in either app launch order.
- Live connection/retry/error states and applied permissions; Windows hardware
  metadata is detected without opening the camera or recording audio.
- Owner-requested model downloads, progress/cancel, hash-verified GGUF catalog,
  and Ollama-first selection. No model is included or downloaded on installation.
- Device configuration through EEF's authenticated node transport. Owners
  approve proposals locally unless they explicitly allow remote management;
  remote requests cannot grant themselves that permission or replace device ID.
- Restart controls for both roles; optional automatic restart after saving
  device settings. An installed update is handed off when Restart is used.
- Local dashboards reject cross-site browser requests and non-loopback Host
  headers. They are local controls, not remotely exposed management servers.
- The old fixed 5 GB floor is removed. Payload space checks remain.

This is not completion of the entire proposed UX/architecture roadmap.
Automatic LAN discovery, per-camera/microphone selection, richer voice/provider
forms, persistent offline-device history, and a larger curated model catalog
remain future work. Offline history currently lasts for the EEF session.
Detection is not proof a media permission or driver works; live recording,
vision inference, Wake-on-LAN, and two-PC failover require device-specific tests.

## Maintenance changes in 0.3.1

- Hash-locked Windows Python runtime dependencies and retained vendor checksums.
- Component and installer Defender scan gates, dependency provenance, and
  per-file bundle inventories; see [ANTIVIRUS.md](ANTIVIRUS.md) for the
  original detection and the limits of the new passing scans.
- Fewer bundled llama.cpp executables, explicit installer launch consent,
  quiet-upgrade startup preservation, and clearer Windows version metadata.
- Isolated installation/upgrade regression coverage for both installers.

This maintenance release does not implement the proposed 0.4 architecture
roadmap or establish that the 0.3.0 detection was a false positive.

## Implemented in 0.3.0

- Rust coordinator and PC node executables in one workspace.
- Normal `node -> EEF -> selected node(s) -> EEF -> originating node` requests.
- Authenticated, encrypted node transport with configurable endpoints and
  priority-ordered reconnect/failover across the configured endpoint list.
- Live capability/model registration and load, latency, capacity, hardware,
  permission, modality, and constraint-aware routing.
- Dependency-aware task plans, concurrent ready subtasks, timeouts, retries,
  cancellation state, structured results, and node reassignment on retry.
- Structured identity, mutable, working, event, conversation, and task state.
- Default-deny node policies for scoped filesystem access, exact allowlisted
  process execution, application control, host/method-limited HTTP, configured
  STT providers, Wake-on-LAN, microphone, audio, TTS, camera, screen, and input
  control.
- Filesystem roots are canonicalized and junction/symlink escapes are rejected;
  public HTTP destinations are DNS-checked and pinned with redirects disabled.
- Owner-selected Ollama and llama.cpp models; no coordinator model and no model
  in the node bundle.
- Optional isolated Python plugin sidecars using the bundled CPython runtime.
- Configurable `off`, `prompt`, and `auto` verified updates for both programs,
  with version handoff and rollback.
- Local EEF and EEFN dashboards for full configuration, capability permission
  switches, and optional startup.
- Two self-extracting Windows installers with per-user defaults, config
  preservation, generated coordinator secret, and a 5 GB storage floor.
- Standalone EEFN operation with zero coordinators; EEF only becomes
  operational when at least one node connects.

## Not yet complete

- Automatic coordinator discovery, quorum election, replicated coordinator
  state, and partition-safe split-brain prevention. Configured node priorities
  already provide deterministic failover when the preferred EEF disconnects.
- A separately authenticated EEFN peer mesh that remains online when every EEF
  coordinator is unavailable.
- Transport plugins beyond TCP. The protocol is not coupled to a VPN vendor,
  but the pluggable transport interface is still future work.
- Recurring jobs, data dependencies for ongoing media workflows, remote execution
  fencing/deduplication, and controlled migration. One-shot interrupted graphs
  are retained locally and require explicit safe resume.
- Signed release manifests. Updates currently require HTTPS plus an exact
  artifact SHA-256.

Reliable coordinator election cannot guarantee both availability and a single
leader during arbitrary network partitions without quorum. The intended next
milestone is a fixed-membership, majority-lease coordinator cluster with a
durable term/vote log, followed by state replication. The mesh is a separate
node role and must not silently promote ordinary nodes into coordinators.
