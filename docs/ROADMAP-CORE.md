# Command-first core roadmap

Owner direction: 2026-09-11. Baseline: published `v0.4.0a`
(`0.4.0-alpha.1`). This is the active roadmap, superseding the sequencing and
future model-install policy in [the original tracker](ROADMAP-0.4.0.md).
It distinguishes planned scope from the development checkpoints below.
Keep the working Rust workspace; extend it incrementally, with migrations and
tests. No rewrite, new release, or model download is implied by this document.

The subsequent owner-requested two-PC test release is tracked separately in
[v0.4.0a2](RELEASE-0.4.0a2.md), internal version `0.4.0-alpha.2`. It packages the
completed increments and explicit diagnostics, not the full roadmap.

## Direction and policy changes

EEF is the control plane: it validates requests, plans, authorizes, and observes
work. EEFN is the node runtime: it interprets natural language when available,
enforces permissions, executes authorized workloads, and reports results.
Authorized nodes may exchange data directly; EEF need not relay every payload.

Pause visual UI development. Prioritize reliable commands, diagnostics,
automation, networking, execution, and model handling. UI changes are limited
to exposing/testing core features, truthful status, security, and accessibility
fixes. Preserve the existing compact UI and copying/editing regressions.

Normal future PC node setup will supply or automatically obtain a replaceable,
CPU-capable lightweight LM. This **supersedes the earlier requirement that new
nodes never obtain a model**. Published v0.4.0a installers still contain/download
no model; do not retroactively change their documentation or artifacts.
EEF itself still needs no local LM. GPU/CUDA is optional, never a core dependency.

## Commands own behavior; UI is a client

The optional [2B interpreter baseline](MODEL-CANDIDATE-EVALUATION.md#completed-2b-baseline-evidence-not-default-approval)
now passes an explicit external verified download and same-PC real CPU lifecycle
checks without model copies or personal selection changes. Both unchanged-prompt
quality suites accept 20/20 schema outputs, but label matches are only 14/20 and
12/20; copied instructional goals occur in 16 and 14 cases. This is failed default
quality evidence, not automatic bootstrap approval, physical acceptance or a new
release. A separately tested field-description prompt regressed clarification and
both label scores; it was rejected and the previous prompt restored, with
[comparison evidence](INTERPRETER-QUALITY.md#rejected-prompt-experiment-field-descriptions-instead-of-sample-answers).

The next unreleased [model-transfer recovery increment](MODEL-TRANSFER-RECOVERY.md)
recovers interrupted pinned GGUF responses within one explicit install job,
with exact remaining-byte ranges, eight attempts, a one-hour transfer budget and
complete SHA-256 verification before publication. Both suites pass 700 tests;
local TLS recovery and existing model-command regressions pass. The optional 2B
interpreter remains experimental: external transfer, quality and default
bootstrap acceptance are not implied, and no installed app or release changes.

The next unreleased [registration receipt increment](REGISTRATION-RECEIPTS.md)
negotiates a nonce/runtime/session-bound acknowledgement over existing encrypted
framing. Setup/status distinguish accepted registration from a local write,
queue readiness and interpreter configuration. Older peers/reports stay unknown,
and receiver retirement suppresses historical accepted labels. It is neither
execution authority nor a liveness lease or complete onboarding/release gate.
Both final suites pass 698 tests; private local enrollment receipt observation,
older report/peer compatibility, blocked-output/no-replay and owner-HTTP boundary
regressions pass locally, with measured scope in the linked document.

The next unreleased [owner HTTP socket-boundary fix](OWNER-API-BOUNDARY.md)
requires real loopback TCP peer metadata before the existing Host/origin checks.
Forged headers no longer suffice on wildcard owner listeners; missing peer
metadata fails closed. The authenticated remote node gateway is unchanged.
OS-user isolation and authenticated remote owner/proxy access remain outside
this fix; a loopback check alone provides neither guarantee.
Both suites pass 694 tests. A real non-loopback IPv4 socket fixture changes the
retained forged-header HTTP 200 baseline to HTTP 403 on both apps, while local
enrollment/credential/setup regressions pass; physical/OS-user scope remains open.

The next unreleased [explicit local enrollment workflow](LOCAL-ENROLLMENT.md)
extends same-PC discovery into owner-approved private preparation, separately
consented/authentication-state-bound coordinator restart, exact applied-state
readback and file-free node pairing. It preserves paused preferences and feature
permissions, refuses different/revoked trust and does not retry writes. It does
not close default model/bootstrap, final registration observation, graphical
installer/onboarding or physical/versioned release acceptance.
Both final suites pass 693 tests; local two-node enrollment, lost-reply/no-replay
and existing credential/setup regressions pass with evidence in the linked doc.
Loopback owner controls remain distinct from the authenticated public node
gateway; network-exposed/multi-user owner-API boundary acceptance is still open.

The next unreleased [same-PC setup plan](LOCAL-SETUP-PLAN.md) provides secret-free
local coordinator discovery and port-free node-owner inspection, independently
of legacy shared-key pairing and the browser. It is read-only; private first-run
provision/apply/handoff and completed registration are not implemented by this
checkpoint and remain required. Both suites pass 687 tests; final headless local
credential/setup fixtures pass with the scoped evidence in the linked document.

The next unreleased [private invitation recovery command](NODE-CREDENTIALS.md#recover-an-existing-private-invitation-unreleased-after-alpha6)
exports existing saved exact-node trust without rotation or writes. It honors
saved revocation even before restart and feeds the existing file-free node pairing
adapter. That adapter waits on bounded read-only local-runtime initialization
before its one pairing submission, rather than failing an early startup race.
This is a resumable setup prerequisite, not automatic first-run
discovery/provisioning, completed pairing or release acceptance. Both final suites
pass 682 tests; two logical-node headless recovery/registration/finite-execution
checks pass locally. The linked credential document records both the retained
startup failure and scoped final evidence; physical acceptance remains open.

The subsequent [input-channel readiness increment](INPUT-CHANNEL-READINESS.md)
installs the local receiver before publishing Connected and separately exposes
unavailable/ready/busy queue observations in status, diagnostics and setup. No
probe, capacity reservation, extra queue or replay is introduced. This addresses
the known publication ordering, not atomic coordinator/model/workload readiness
or completion of physical/onboarding/release acceptance. Both suites pass 677
tests; headless setup/permission checks and a blocked-output reconnect regression
pass locally, with measured scope in the linked document.

The following unreleased [feature-permission commands](FEATURE-PERMISSIONS.md)
expose whole-feature allow/deny/inspection through the same node services and
current owner-approved coordinator channel. Legacy scopes/limits survive,
stale runtime/digest and revoked remote approval reject writes, and saving is
separate from applying or authorizing work. This improves command-based setup;
it does not finish first-run automation, media availability, strong Stop or
physical/versioned release acceptance. Both final suites pass 674 tests; the
headless public acceptance observes real reviewed file work and 254 ms contained
native cleanup on permission loss. A startup-wait fixture failure is retained,
and the existing connected-before-mailbox publication window remains a separate
reliability follow-up, as recorded in the linked document.

The unreleased [plain-language setup check](SETUP-CHECK.md) explains the running
node's connection, private receipt configuration, saved/applied interpreter and
pending settings through one shared command service. It uses the actual
interpretation selector, keeps unavailable observations explicit and never
infers, mutates or grants execution. It improves understanding of incomplete
setup, not automatic private pairing, verified/default bootstrap, model quality,
the setup wizard or physical/versioned release acceptance. The final suites pass
665 tests per feature mode; headless setup and existing preview regressions pass
locally, as recorded in the linked document.

The subsequent unreleased [selected-GGUF lifecycle](MODEL-LIFECYCLE.md) provides
runtime-owned per-selection load/unload/status/cancel commands through the node
and approved EEF owner adapters. Exact runtime/generation receipts prevent
replay; active inference pins block unload; current owner authority and load
ownership are checked at application. The existing Windows process-tree guard
also contains GGUF children. This does not finish full lifecycle, general local
pipelines, default interpreter bootstrap or physical/versioned release acceptance.
The following [owner policy](MODEL-LIFECYCLE-POLICY.md) adds configurable eager/
manual startup, operation bounds and opt-in monotonic idle unload, preserving
default behavior and saved/applied separation through command services.
An additional owned-GGUF on-demand path loads only for the requesting operation,
hands health readiness directly to an inference pin, and retains deadline/failure
uncertainty without implicit retry. Cold availability remains distinct from
readiness. Actual resident metadata now feeds the existing warm routing score.
Shared Ollama requests now have a command-configurable retention preference
(inherit by default, bounded duration or explicit indefinite retention), separate
from owned process control. It does not imply exclusive/shared backend ownership,
observed residency or autonomous unload. Full Ollama lifecycle, broader per-model policy, measured resource admission
and startup-cost scheduling remain open. The dashboard owns none of this behavior.

The next unreleased increment adds owner thinking request hints (inherit/default,
disabled, enabled) through the same command policy, exact runtime/CAS and remote
approval checks. Saved changes apply only on explicit restart; no backend/model
name is special-cased and inference callers cannot override the owner hint.
Real CPU lifecycle acceptance passes with an optional pinned Qwen 3.5 0.8B text
artifact and disabled hint, but the unchanged interpretation rubric scores only
8/20 matches. It remains experimental, not the automatic default; stronger
quality/bootstrap and physical acceptance are still required. See [measured
results and remaining scope](INTERPRETER-QUALITY.md#qwen-35-candidate-and-thinking-request-hints-2026-10-14).

The subsequent [lightweight telemetry increment](HEARTBEAT-TELEMETRY.md) removes
unused process-table scans from node heartbeats and keeps OS queries off async
workers. Fresh public finite recurrence passes, but ordinary scheduling still
fails a later activation with confirmed stale heartbeat age and zero issued
attempts. Journal/runtime responsiveness remains an open reliability gate; no
freshness, expiry, replay or authority fence has been relaxed.

The next [owned timer checkpoint worker](AUTOMATION-SCHEDULES.md#owned-periodic-journal-worker-subsequent-increment)
keeps periodic journal processing off async workers and drains even queued
callbacks during shutdown, without replay or new execution authority. The
unchanged ordinary schedule and finite/native public regressions pass locally;
full suites pass 612 tests in each feature mode. Other journal paths, loaded-
system/physical acceptance, strong stop and the remaining roadmap stay open.

The next [owned schedule inspection increment](AUTOMATION-SCHEDULES.md#read-only-command-inspection-worker-subsequent-increment)
keeps read-only Show/List off async workers with a separate single-flight slot,
drains cancelled/queued callbacks during shutdown and reads the exact linked job
with its schedule under one validated snapshot. Fresh full suites pass **615
tests** per feature mode. Public recurrence and commit-time timing failures
remain open; unchanged failed evidence is retained. Synchronous grant-report
journaling under the shared pending-request mutex is a confirmed next code path
to address at that checkpoint, not an established complete failure cause. No release, installed
node update, UI work or relaxation of authorization/deadline fences follows.

The subsequent [owned grant-report callbacks](GRANT-REPORT-JOURNAL.md) move
durable report saves off async workers and out of the shared correlation lock.
Late ACKs are privately fenced; bounded callback/result ownership is drained
even after waiter/Stop cancellation. Full suites pass **623 tests** per feature
mode, and fresh public debug scheduling, finite recurrence and native process
checks pass locally. Fresh optimized binaries/examples and the same public
release checks also pass, as recorded in the linked report-worker document. Earlier
failures, same-session frame blocking, other synchronous paths and physical/load
acceptance remain open; no stable release or installed-node update is implied.

The next [framed-session responsiveness increment](GRANT-REPORT-JOURNAL.md#framed-session-responsiveness-follow-up-unreleased)
keeps heartbeats/control replies flowing on a node's connection during journal
saves, with one session-owned FIFO consumer, count/serialized-byte bounds and
unchanged processing-time ACK/order/lease fences. Malformed/overloaded sessions
retire uncertain; started saves remain owned. Full suites pass **631 tests** per
feature mode and fresh debug public scheduling/pipeline/native checks pass.
Optimized binaries/helper build; ordinary/native tests pass, while the pipeline
runner has another unexplained `0xc0000409` exit before any retained job.
Private phase diagnostics and a verified portable runner produce a subsequent
passing pipeline check, without proving that original crash's complete cause.
Both observations and the still-open runner/physical/load acceptance are recorded
in the linked document. Other synchronous paths,
physical/load acceptance and the complete bootstrap/media/resource/peer/cluster/
installer/security requirements remain open before stable 0.4.0.

The next [interpreter-quality investigation](INTERPRETER-QUALITY.md#further-rejected-variants-2026-10-14)
evaluates four prompt/generation-order variants through the real isolated CPU
node using the existing 1.5B artifact. None meets bootstrap quality; every
experimental production change is restored rather than approved from accepted
JSON or a partial label score. Full prompt-sequence references, matching binary
hashes and exclusive private capture improve reproducibility without inference
or execution. Restored full suites still pass **631 tests** per feature mode and
public preview/review regressions pass. Stronger quality/held-out/physical evidence
and automatic verified bootstrap remain required; no model, node or release update
is implied, and the complete roadmap scope remains unchanged.

Another definition-only prompt trial on the optional 0.8B candidate regresses to
zero matches and is restored completely. An additional case set frozen before
its first evaluation scores only 7/20 with the restored production prompt.
Known-case-set/hash checks and metadata-only reference capture tests are retained;
no model/prompt default or production behavior changes. These [negative results](INTERPRETER-QUALITY.md#rejected-definition-only-prompt-and-additional-cases-2026-10-14)
do not close interpreter/bootstrap or independent physical acceptance.

A subsequent [formatting-only diagnostic](INTERPRETER-QUALITY.md#formatting-only-backend-diagnostic-2026-10-14)
uses the shared bounded prompt/schema reference and an isolated node-owned CPU
runtime, without generation or a coordinator. It confirms preserved reference
text and thinking-prefix changes in the pinned backend, but is not actual-wire
capture, semantic quality or remote attestation. Both feature modes pass 639
tests and the public preview regression passes. Controlled sampling/candidate
evaluation is still needed; no default or installed-node change is implied.

The next unreleased increment exposes bounded owner interpreter sampling through
the same policy commands and runtime/revision fences. Integer-thousandth storage
keeps old default digests exact; optional hints are applied only to interpretation,
clarification and method preparation after explicit restart, not generic work.
Headless approval/saved-applied/reset checks pass, but a real sampled profile
scores only 4/20 original and 5/20 additional matches and is not made default.
See [measured scope and remaining gates](INTERPRETER-QUALITY.md#owner-interpreter-sampling-and-rejected-sampled-profile-2026-10-14).
Per-model overrides and full bootstrap/physical acceptance remain open.

A following [owned-GGUF memory ceiling](MODEL-MEMORY-LIMITS.md) adds optional
Windows job-commit enforcement before model code resumes, bounded owner commands
and honest saved/runtime registration fields. Native descendant allocation tests
and real CPU error/reset/restart acceptance pass locally. This is not working-set
RAM, VRAM, shared-Ollama control, aggregate resource admission or physical-network
acceptance; the full resource/lifecycle/bootstrap/release gates remain open.

The next [owned-job accounting increment](OWNED-JOB-ACCOUNTING.md) reports native
Windows job ceiling/peak commit, process count and cumulative CPU ticks through
existing lifecycle status. Missing, unsupported and failed observations stay
explicit, and samples are not routing/admission authority or current RAM/VRAM
measurements. Old snapshots remain compatible; complete measured admission,
cross-platform and physical/bootstrap/release requirements remain open. Both
feature-mode suites pass 660 tests; public lifecycle and cached real CPU-model
checks pass locally, with detailed scoped evidence in the linked document.

The unreleased [explicit result recovery](RESULT-RECOVERY.md) increment reconciles
an idle interrupted reviewed attempt from exact retained completed evidence,
preserving original uncertainty/ACKs and preventing implicit continuation. It
extends the existing durable job system, not a parallel runtime. Read-only result
inspection remains unchanged. Automatic reconciliation, full safe continuation,
strong termination and automatic continuation remain open; localhost tests
do not replace physical acceptance or complete the roadmap.

The next unreleased [execution boundary](EXECUTION-BOUNDARY.md) makes reviewed
grants mandatory in the public EEF core and on the node's ordinary RPC receiver.
It retires legacy workload admission/dispatch/resume, moves `--ask` and node
HTTP input to durable review, and migrates text-job creation to review/compilation.
No second node runtime, model/RPC fallback or configuration opt-out is introduced.
The following checkpoints cover client/regression migration and initial automatic
source-bound preparation. Answer workflows and physical acceptance remain open;
this is not finished first-time automation.

The following unreleased [client acceptance checkpoint](CLIENT-ACCEPTANCE.md)
migrates model-management, browser and real-model regressions to reviewed grants.
It fixes last-workload diagnostics and adds actionable interpreter setup guidance,
without a visual UI redesign or automatic role selection. It exposes two
remaining onboarding requirements: owner-pinned grant pairing and interpreter-role
configuration must become a usable non-coder command/client flow. Passing these
fixtures with explicit setup is not completion of that requirement or stable 0.4.0.

The next unreleased [input workflow](INPUT-WORKFLOW.md) shares normal CLI/HTTP
input with a runtime-owned preparation policy. Fresh configurations can prepare
eligible one-shot requests awaiting approval; older missing policies remain
review-only. It preserves complete source lineage and separate receipts, fails
busy without queueing, and prevents implicit continuation after uncertainty or
runtime change. Reusing input only inspects origin-owned history. Owner approval,
execution, answer-triggered workflows, durable automation and reusable bounded
approvals are not supplied by preparation policy; the full release scope remains.

The subsequent [answer workflow](ANSWER-WORKFLOW.md) adds a separate applied
resolution policy and explicit target/revision input through shared node commands.
EEF records new answers' exact source revision/digest atomically; legacy unknown
evidence stays unknown. Bounded resolution/preparation keeps original history and
awaits approval, while partial/reused receipts cannot trigger implicit continuation.
This covers initial answer triggers, not durable automation definitions, reusable
approval policy, general planning, complete onboarding or physical acceptance.

The next [explicit interpreter setup](INTERPRETER-SETUP.md) increment lets the
owner move only the interpreter role to one existing selection atomically through
node or approved coordinator commands. Other roles/restrictions survive, saved
and applied models stay separate, and no model/provider is chosen automatically.
This removes multi-write hint editing, not the default bootstrap/quality or
friendly private pairing requirements. Complete non-coder setup remains open.

The next [production interpreter evaluation](INTERPRETER-QUALITY.md) adds a fixed
twenty-case semantic rubric through the actual node preview path, plus bounded
backend JSON generation hints. The tested CPU candidate accepts 10/20 proposals
but matches only 3/20 semantic expectations; it remains unapproved as a default.
Formatting hints never replace independent validation, approval or execution
policy. This is quality evidence, not completion of bootstrap or automation.

The follow-up adds an optional exact-artifact 1.5B manual catalog choice and
command-driven real download/inference evidence. It accepts 18/20 proposals and
matches 12/20 labels, but inspection finds copied placeholder goals/questions;
two value-free prompt experiments regress semantic labels and are reverted.
Neither candidate is approved as a default. The production harness now records
a shared-prompt reference hash. Automatic bootstrap, held-out faithful-language
evaluation and physical resource acceptance remain open; no UI, installed node
or release/feed mutation occurs.

The subsequent [file-free private pairing input](NODE-CREDENTIALS.md#file-free-invitation-input-unreleased-after-alpha6)
adds a shared exact-node/runtime command for private API bodies and bounded piped
CLI input. It preserves identity, permissions, models and paused preference,
separates saved/restart-requested from completion, and rejects unconfirmed replies
without fallback/replay or secret echo. Existing file pairing remains compatible.
This removes an intermediate invitation file for clients/automation, not owner
provisioning, automatic private same-PC pairing, bootstrap or complete non-coder
onboarding. UI assets and installed nodes are unchanged.

The subsequent [local-model CPU settings](MODEL-CPU-THREADS.md) provide per-model
bounded worker configuration through the shared node/approved coordinator
commands. Saving preserves roles/files/provider and differs from applying;
reselection cannot silently clear the count. The launcher configures generation
and prompt workers together and passes explicit zero GPU layers for the CPU
choice. These are backend settings, not CPU-percent/RAM quotas or full workload
budgets. Default bootstrap, native termination, measured physical resource limits
and all other release gates remain open.

The subsequent [bounded node output](TRANSPORT-OUTPUT.md) increment applies one
existing-session write guard to ordinary replies, heartbeats and all remaining
node frames. Failed/partial output retires the connection before another frame
can append; reconnection does not replay mutations, grants or submissions.
Outgoing encoded frames obey the existing receiver limit. Real TCP fault and
actual headless-node command fixtures cover retirement/reconnection, not native
stop, physical two-PC behavior, the intermittent busy reply or automation.

The next [durable automation definition](AUTOMATION-DEFINITIONS.md) increment
adds exact compiled-source authoring/inspection/removal through shared node
commands, inside the existing job journal and quotas. Definitions stay disabled;
compact removal receipts prevent creation-ID reuse from recreating them. Source
snapshot retention is not reusable approval, current executor readiness or a
timer. Bounded owner authorization, atomic occurrence admission, safe triggering,
overlap/restart/missed-trigger handling and full physical execution remain open.

The follow-up adds an `eef automations` local-owner CLI adapter for the same
disabled-definition service, preserving node-origin lineage and exact receipts.
Lost/corrupt/negative writes are inspected rather than replayed; this is command
coverage, not reusable authorization or scheduling. A separate checkpoint
diagnostic increment preserves bounded private coordinator rejection text while
keeping possible writes uncertain. The prior intermittent preparation failure
remains unproven; successful reruns are not treated as its fix.

The next [reusable owner-approval checkpoint](AUTOMATION-APPROVALS.md) retains
fixed scopes, current issuer/runtime identity, expiry, generation, run/grant and
aggregate duration/result bounds. It adds owner commands and shared storage
accounting with reserved revocation headroom. Superseded, revoked, removed or
old-runtime policies cannot silently become current. Exact receipt reuse is
inspection, not renewal. This stage does not consume budgets or admit/approve
occurrences; atomic independently compiled-job delegation, every-issuance fences,
safe triggering/overlap/restart behavior and full physical execution remain open.

The subsequent [explicit occurrence reservation](AUTOMATION-OCCURRENCES.md)
connects reusable approval to independently compiled matching jobs in one
transaction. Retained reservations consume run and worst-case duration/result
allowances without refund. Current policy authority is checked at start and
every grant issuance under the journal/admission gates. Existing durable grants,
node enforcement and explicit recovery remain the only execution path. Original
source history is not copied or rewritten; overlapping/uncertain previous work
cannot silently create replacement occurrences. This adds manual owner admission,
not timers, automatic job creation, terminal archiving, strong native termination,
measured full resource quotas or completed recurring automation.

The subsequent explicit terminal archive command atomically retains validated
completed or unissued-cancelled evidence and removes the live job. Budget
consumption, immutable reservation history and original request tombstones remain;
restart or lost responses cannot recreate work. It shares count/byte accounting
and refuses stale, damaged, future or unresolved evidence. Full snapshots are
bounded to 2 MiB; payload compaction, automated retention and partial-work
resolution remain open. This improves the durable lifecycle foundation, without
claiming timer activation or the full recurring-automation milestone.

The next [durable trigger foundation](AUTOMATION-TRIGGERS.md) adds internal
schedule/cursor/claim state in the same journal, with preallocated fresh request/
reservation/execution IDs, atomic due claims, fixed-delay/no-catch-up semantics,
restart interruption and stop/issuance authority fences. Terminal resolution uses
actual retained or archived occurrence proof; missing work remains uncertain.
Shared quotas include stop/interruption headroom. Public enable/stop adapters,
the runtime timer task and fresh ordinary request/compile/reserve/dispatch bridge
remain unimplemented: no timer or automatic job is enabled by this checkpoint.
Monotonic suspend/clock detection, partial-claim recovery, physical execution
acceptance and the full recurring-automation milestone remain open.

The following internal fresh-admission bridge creates ordinary explicit requests
and uses existing compilation/reservation/execution, with a durable one-shot
launch marker and current origin/provider checks. Late source admission or
compilation is fenced after stop/policy change; stale callbacks cannot interrupt
successor claims. Actual retained unapproved cancellation can resolve partial
admission without inventing reservation or completion evidence. Public owner
adapters, runtime timer integration and native scheduled-workflow acceptance are
still absent; this service is not publicly activated and does not complete the
recurring-automation milestone. See [bridge limits/evidence](AUTOMATION-TRIGGERS.md#internal-fresh-admission-bridge-subsequent-increment).

The subsequent [automatic schedule increment](AUTOMATION-SCHEDULES.md) connects
guarded owner enable/stop/inspection to a bounded runtime timer task and fresh
ordinary compilation/reservation/execution. Local native checks demonstrate three
automatic system-information jobs, one activation under concurrency, stop after
completion, revocation/no-next-run, restart/no-rearm and uncertain reply reuse.
Original source evidence and permanent policy budgets remain. This is fixed-input
one-template scheduling, not general semantic automation or completed durable/
continuous workloads. Partial-source/no-work recovery, full resource/native-stop
and physical acceptance, peer/media/cluster, bootstrap/non-coder onboarding and
installer/signing gates remain open. UI work remains paused.

The subsequent proof-bound pre-launch refusal recovery adds an owner command
that resolves only actual retained backend refusals before the one-shot launch
marker. Missing jobs, partial admission and issued uncertainty remain blocked;
resolution does not re-arm, replay or refund reserved budgets. A local native
origin-disconnect/reconnect fixture proves resolution and separate fresh owner
activation. An independent bounded clock monitor also avoids treating ordinary
journal processing as a suspend gap without relaxing safety thresholds. This
advances durable automation reliability, not general continuation or stable
release acceptance. See [contract and evidence](AUTOMATION-SCHEDULES.md#explicit-pre-launch-refusal-recovery-subsequent-increment).

The following [bounded process lifetime](PROCESS-LIFETIME.md) increment contains
Windows `process.exec` descendants before resuming user code and owns readers
within the execution future. Completion/timeout/cancellation cannot intentionally
leave persistent children behind; grant uncertainty and replay refusal remain.
This advances one native lifetime boundary, not universal process sandboxing,
remote-stop acknowledgement, full physical resource enforcement, Python/model/
media lifecycle acceptance or the completed workload-runtime milestone.

The following public-path lifetime acceptance now exercises six native command
scenarios through separate origin/executor logical nodes, explicit disclosure,
source compilation, owner approval, grant issuance and retained result inspection.
Held creation-verified handles observe both root/child exit on completion,
timeout, deadline, local permission loss and either-role restart. Non-normal
results remain unknown and exact receipts do not replay. This closes a local
reviewed Windows process acceptance gap, not physical two-PC, universal native
stop/migration, full resource/pipeline/media/model or release acceptance.

The next explicit process authoring increment adds `eefn requests submit-process`
with typed literal options rather than a JSON file. It still uses the existing
review/compiler/owner approval/grant path. Native acceptance exposed a nonzero
exit incorrectly treated as successful; the node now retains an unsuccessful
completed report and EEF records a failed job. Exact-grant confirmation rejects
contradictory false-success native evidence without rewriting old history.
This advances command usability and failure-aware execution, not automatic
code upgrades, universal stop acknowledgement, general pipelines or stable 0.4.0.

The 2026-10-06 unreleased diagnostic increment distinguishes authenticated
connections from gateway-accepted registration and scheduler provider counts.
It addresses the observed reconnect-before-provider race in troubleshooting;
it does not gate submissions, retry failed work or guarantee execution readiness.
See [diagnostic semantics](COMMANDS.md#diagnostics-and-private-pairing).

The unreleased [backend/browser boundary](BACKEND-BOUNDARY.md) now separates both
roles' optional browser assets from their independently operable command/API adapters.
It adds API-only startup and no-dashboard builds, node connection commands, and
independent saved browser preferences while preserving legacy listener settings.

```text
CLI / automation / dashboard -> shared typed commands -> policy + core services
natural language -> node interpreter -> validated request -> EEF plan + grant
                                                          -> node execution
authorized node <---------------- bounded data ----------------> authorized node
                          status/events/results -> EEF -> origin node
```

Commands are structured application operations, not arbitrary shell strings.
Natural language is an optional input adapter, not required for deterministic
commands. Both paths pass the same authorization and validation boundaries.

- Put configuration validation, model operations, connection control, job
  lifecycle, permissions and listening state in reusable Rust services. CLI,
  HTTP and UI call those services; no business rules in browser code or a
  dashboard-owned object required to operate a node.
- Keep local command/IPC services available without serving the browser UI.
  Discover the running instance through existing per-user discovery; ordinary
  commands must not need a port number or create a second node connection.
- Provide readable help/output and versioned JSON output, stable error codes,
  exit codes, correlation/operation IDs and status/event inspection. Keep logs
  separate from machine-readable stdout. Test Windows console and piped output.
- Long operations return durable IDs where applicable; command timeout or a
  closed UI does not imply remote cancellation. Report requested, acknowledged,
  stopping and stopped states distinctly. Poll/watch reconnect does not resubmit.
- Mutations require the existing local owner or approved remote authority.
  Scripts fail with `approval_required` rather than hanging for a hidden prompt.
  Reusable approvals are explicit and bounded; no blanket approval implied by
  installing the app, selecting a model, or writing an automation.
- Retain command identity/deduplication in the existing durable job journal for
  workload submissions. Do not promise exactly-once side effects. Configuration
  commands retain saved/applied separation and atomic validation.
- Initial command families: status/diagnose, connection, identity/address,
  network inspection, models, requests, jobs, permissions, listening, restart
  and updates. This names planned interfaces, not new executable syntax.

## Mapping onto v0.4.0a

Subsequent unreleased command increments now include model selections locally
and through approved EEF control, explicit role-constrained text/vision routing,
and node-origin job CLI controls with bounded text-output inspection. These reuse
the existing services, submission connection and durable journal. They do not
complete automatic interpretation, model bootstrap, execution grants, direct-peer
authorization or clustered coordinator safety. See [commands](COMMANDS.md).

Job creation receipts now correlate CLI attempts with persisted origin context;
`jobs find` searches retained origin-scoped history after uncertain replies.
Reply-routing IDs are independent and no mutation is automatically replayed.
The 2026-10-06 follow-up adds atomic node-origin creation deduplication while the
original job remains in history: identical receipts/goals reuse it, conflicting
or legacy-ambiguous receipts fail closed, and interrupted work is never resumed
implicitly. This survives restart but not history removal/database replacement.
Durable request-review openings and answer/proposal receipts now reuse this same
journal, retaining original text/context and clarification history without a
runner. Other commands still have correlation only. A permanent receipt ledger, execution
grants and exactly-once effects are not claimed; see [commands](COMMANDS.md).

The 2026-09-26 unreleased reliability increment serializes job admission with
engine shutdown: new-job persistence and runner registration share the closing
gate, as does resume checkpointing. Rejected submissions cannot leave orphan
pending jobs after shutdown. This is a single-process lifecycle boundary, not
durable request deduplication, an execution grant or remote cancellation.

The 2026-09-27 follow-up places Stop's runner-presence check/checkpoint and runner
retirement under that same gate. A racing Stop cannot leave an unowned job stuck
in `stopping`; uncertain exits remain interrupted. This is still local journal/
runner coordination, not an executor acknowledgement or remote-stop guarantee.

The 2026-10-06 gateway follow-up owns and drains connection handlers and their
socket writers during shutdown, including incomplete authentication. Old identity
leases are released before restart, pending replies fail, and interrupted cleanup
can finish without overlapping gateway generations. This is transport retirement,
not cancellation of remote effects or completion of execution-lease work.

The 2026-10-09 reply-path follow-up rejects saturated response queues immediately
instead of blocking the shared event listener. Missing acknowledgements remain
uncertain; accepted work is neither rolled back nor automatically replayed.

The subsequent session-binding checkpoint ties RPC waiters, response matching,
disconnect cleanup and originating submission replies to a gateway-owned
connection generation. A replacement connection with the same node ID cannot
receive an old completion, even if it deliberately reuses a transport ID.
Waiter registration is serialized with route retirement. This is ephemeral reply
ownership, not durable execution grants, issuer epochs or node-side replay fencing.

The completed node-side admission slice bounds incoming execution futures with configurable
work/control allowances. Ping/status/approved restart have separate reserved
capacity; excess work reports not-started without reaching the executor. Normal
permission checks remain mandatory. This is an admission prerequisite for local
workloads, not a CPU/RAM budget, continuous runner, grant or remote-effect limit.

The subsequent coordinator reservation slice makes non-model provider selection
and in-flight accounting atomic. Registration retirement isolates late releases;
cached gateway receipt times no longer become fresh during reconciliation. Stale
providers and malformed known targets are rejected, and dispatch validates the
selected provider's schema. This is coordinator-local admission, not a remote
execution acknowledgement, model-route reservation, physical budget or grant.

The 2026-10-10 follow-up reconciles live registration/model/metric state after
gateway event loss. Observer loops survive broadcast gaps, report their losses,
and suspend world-condition automation when its observations may be stale.
State repair does not replay lifecycle triggers or user submissions. Full world
reconstruction and acknowledged resource-scoped execution remain future work.

The next 2026-10-10 checkpoint keeps full remote restart inspection/polling in
reserved status capacity on updated nodes, with application-time owner approval
and read-only compatibility for older node status contracts. Saturated model work
does not block this tested command path; saturated control capacity still rejects
without retry. This is app restart, not acknowledged cancellation of remote effects.

Inspected source, not just the earlier proposal:

| Concept | Existing implementation | Required extension |
| --- | --- | --- |
| Identity and configuration | `eefn/src/setup.rs`, `main.rs::FileConfig`, `client.rs::NodeClientConfig` | Preserve persisted IDs/name; add separately validated advertised endpoints and config migration. |
| Discovery and node view | `eefn/src/protocol.rs`, `server.rs::ConnectionInfo`, `client.rs`; `eef/src/runtime.rs`, `world.rs` | Current socket source address is diagnostic, not a peer listener address. Add explicit registrations, freshness and authorized lookup. |
| Origin/areas/resources | `eefn/src/context.rs::RequestContext`, `NodeMetadata`, `Resource` | Preserve server-stamped origin and private resource bindings; new hints never replace them. |
| Commands/input | `eefn/src/main.rs::Args`, `submission.rs::SubmissionMailbox`, `dashboard.rs`; `eef/src/api.rs` | Existing `--ask` and bounded authenticated submission are reusable. Extract service state and operations from HTTP/dashboard ownership. |
| Model inventory | `eefn/src/client.rs::SelectedModel`, `model_manager.rs`, `model_server.rs::ModelSlot`; `eef/src/model.rs::ModelSpec`, `ModelRegistry` | Lists already exist; generalize the text/VLM modality schema into multiple capabilities/modalities/roles, lifecycle and unknown resource metadata. |
| Planning and durable jobs | `eef/src/task.rs::Goal`, `Plan`, `Dispatcher`, `TaskEngine`; `jobs.rs::JobStore`, `AttemptJournal` | Extend existing identities, attempts and journal for grants, executor acknowledgements and continuous work. No parallel job framework. |
| Enforcement and adapters | `eefn/src/engine.rs::NodeEngine`, `NodePolicy`, `python.rs` | Check each granted operation locally; add bounded local runners and separately authorized peer sessions. |
| Model process lifetime | `eefn/src/model_server.rs::ModelServer`; `eef/src/model.rs::ModelStatus` | Existing startup launches configured GGUF slots; warm/unloaded/unavailable is not a complete measured lifecycle. Add on-demand ownership, pinning and idle unload. |

Paths above are relative to `crates/`. At the v0.4.0a baseline,
`model_manager.rs` depended on `NodeDashboard`, which also owned shared live state
and submissions. The first development checkpoint below moves that ownership
into runtime services while preserving HTTP routes and approval rules. An HTTP
API alone does not satisfy the target separation.

## Implementation order and acceptance

Command/service separation is a cross-cutting requirement from the first slice,
not a separate rewrite prerequisite. Each row must compile and remain compatible
before proceeding. "Planned" includes extensions to working baseline components.

| Order | Increment | Acceptance boundary | State |
| --- | --- | --- | --- |
| 1 | Identity + advertised address | Local coordinator connection and different advertised address coexist; rename/address change preserves identity; command config/readback; old configs migrate. | Implemented in development; one-PC command tests passed; physical acceptance pending |
| 2 | Coordinator advertisements + network view | Authenticated, bounded, fresh node listings and coordinator hints; authorized peer lookup; no automatic trust or claimed cluster election. | Scoped pull discovery, registration metadata and freshness implemented; owner grant/revoke and restart CLI added in development; no direct peer transport claim |
| 3 | General `models[]` | Multiple multi-capability models per node/backend; normalized old selections; capability-based scheduling; unsupported metadata stays unknown. | Versioned metadata/inventory and node selection commands added in development; text/vision routing filters capabilities/state and explicit owner role constraints; route inspection available; automatic role-based planning and new executors remain planned |
| 4 | Default lightweight LM | Configurable verified bootstrap; CPU inference; no manual model choice needed in normal setup; offline/disk/cancel failure leaves core usable. | In progress: verified artifact/preflight and command downloads; real CPU candidate benchmark added, but candidate not approved and automatic bootstrap not implemented |
| 5 | Structured request interpreter | Versioned bounded schema; original text retained; invalid/time-limited model output rejected; explicit commands work without a model. | Partial in development: strict contract, origin-owned durable history, fresh cross-node vocabulary and source-bound method/questions. Normal input and explicit-target answers follow configurable bounded preparation/resolution policy, without approval/execution or implicit continuation. Offline preview stays local. Real-model quality, bootstrap and complete non-coder setup remain required |
| 6 | Planning + execution authorization | Existing durable jobs persist plan/grant before dispatch; scope enforced by executor; observable acknowledgements; expiry/replay/ownership tests. | Partial in development: origin-scoped compile/plan inspection, source-bound preparation/questions, immutable explicit inputs, separate owner approval/execution, exact grants and durable issuance/ACK/completion, replay protection, clock-independent leases, restart-to-unknown, explicit abandonment and retained-completion reconciliation without continuation. Public core and node ordinary RPC now fence legacy workloads; text creation uses reviewed compilation. Origin and executor may differ. Automatic triggers/heavy planning, complete client/regression migration, general continuation, stronger termination, physical acceptance and cluster fencing remain required |
| 7 | Local workload runtime | Authorized bounded pipelines execute locally with progress, pause/stop acknowledgement, resource limits and safe restart reconciliation. | Partial, unreleased: explicit reviewed finite graphs execute on one fixed node under one parent grant/deadline, local data bounds and atomic child-provider reservations; bounded recurrence reuses existing frozen policies and timers. One-shot ACK/completion, read-only recovery and retained-completion reconciliation preserve no replay. Continuous pipelines, durable progress, acknowledged strong stop and physical resource budgets remain open |
| 8 | Authorized peer data paths | Pair-specific authentication, scoped transfer limits and expiry; address knowledge alone cannot open a session. | Planned |
| 9 | Continuous workloads + listening | Local camera/audio/sensor loops, event reporting and backpressure; repeat payloads stay local; listening-off leaves services working. | Planned |
| 10 | Full model lifecycle + unload | Measured transitions, load cancellation, active-use pinning and configurable idle unload; scheduler considers startup cost and actual usage. | Partial, unreleased: selected owned-GGUF load/unload/status/cancel, exact runtime/generation receipts, active inference pins, scoped native lifetime, command-configurable eager/manual/on-demand startup, opt-in monotonic idle unload and shared Ollama request retention preferences. Full Ollama ownership/lifecycle, broader per-model policy, measured resources/startup-cost scheduling and physical acceptance remain required |

The development increment after v0.4.0a3 runs local GGUF startup concurrently
with node registration/control, reports bounded group readiness/error metadata,
and cancels startup children on runtime restart. This is a reliability prerequisite
for bootstrap, not completion of default model selection or full lifecycle work.

A follow-up scoped process monitor now withdraws the local group after an owned
child exits following readiness and stops surviving siblings. Commands stay
available, including while disconnected, and explicit restart retries the saved
selection. This is process-exit observation, not ongoing inference/HTTP health,
automatic recovery, per-slot lifecycle control or completion of step 10.

The next command increment exposes the existing installed-model browser,
explicit model installation and exact receipt progress/cancellation through the
running node's CLI/API. It retains the current transfer service, provider policy,
verification and disk limits. Receipts are only the latest in-memory transfer,
not durable bootstrap state. Default artifact selection, verified bootstrap
manifest policy and automatic interpreter role binding remain planned.

Install preflight now normalizes GGUF artifact metadata into a versioned contract
shared with the installer and reports backend-specific storage/verification
limits before transfer admission. It does not select a default, perform license
review, establish runtime compatibility or authorize automatic bootstrap.

The [candidate evaluation](MODEL-CANDIDATE-EVALUATION.md) now records pinned
artifact verification and real CPU startup/memory/classification measurements.
The detailed baseline matched 9/12 synthetic cases but failed ambiguity and
instruction-override cases; no automatic default was approved. Strict model-output
validation and independent policy boundaries are required before integration.

Minimal load/readiness/error reporting is needed in steps 3-7; do not postpone
basic execution correctness until step 10. Step 10 adds complete lifecycle
control and optimization, rather than enabling the first usable inference.

## 1-2. Identity, addresses and discovery

Keep `node_id`, mutable display name, and advertised address separate. Retain
existing `device-*` IDs: the proposed `node-*` example is not grounds to reset
identity, job ownership or resources. Preserve the existing wire `name` field
through compatible mapping if the public contract calls it `display_name`.

An outgoing EEF endpoint, bind/listen endpoint, observed socket source, and
advertised reachable endpoint are distinct facts. Do not turn an outgoing
connection's ephemeral source port into a peer service port. Validate IPv4,
IPv6 and hostnames, with explicit service ports where needed; an address alone
does not imply an active listener. No Radmin/Playit/Hamachi/LAN protocol modes.
Radmin is the owner's preferred PC deployment example, not a runtime dependency
or a claim that the app configures an overlay/tunnel automatically.

Support a node connecting to local EEF via `127.0.0.1:51335` while advertising an
owner-selected overlay address. Never substitute that loopback address for a
remote peer endpoint. Reachability is requester/path-specific, time-bounded and
distinct from authenticated identity. Keep the existing EEF pre-connect probe;
successful probing does not authorize operations.

An optional coordinator advertisement contains coordinator identity, advertised
service endpoint, reported active/standby/unknown state and freshness. It is
discovery metadata, not proof of leadership or authority to change trust roots.
Newly learned coordinators still need the existing owner-approved trust/pairing
path. Do not implement silent failover by trusting a registration string.

EEF's node view includes identity/name, advertised endpoints, online/stale
status, capabilities, models and optional coordinator presence. Update it from
authenticated registration/heartbeats, bound collection sizes, expire stale
records, and define compatible defaults for older nodes. Disclose only fields
needed by the requesting authority; do not distribute PSKs, adapter paths,
tokens or an unrestricted address directory. Address updates alone must not
open connections to arbitrary targets: validate endpoint scope and use bounded,
authorized reachability checks, including DNS resolution/rebinding handling.

## 3-5. Models and interpretation

Extend the current inventory instead of inventing one fixed slot per model type.
Identify a model instance by node, backend and backend model ID; preserve useful
display names. Separate capabilities, input/output modalities, optional roles,
availability, lifecycle and known resource estimates. Examples include language,
vision, detection, OCR, STT, TTS, embedding and classification. One model can
provide several capabilities; advertising metadata does not implement a backend
or grant device access. Map legacy text/VLM selections without widening grants.

The lightweight LM is a configurable model role, not a hardcoded model ID.
Reuse a suitable owner-selected installed model where possible; otherwise use
a versioned bootstrap manifest with source, license, size, digest and runtime
requirements. Select the actual default artifact only after license review and
CPU/startup/memory/interpretation tests. Prefer existing reachable Ollama and
retain local GGUF/llama.cpp; never require CUDA or install a second provider
unnecessarily. The manifest and role binding must be replaceable.

Normal PC setup should automatically bootstrap the default with clear download
size/progress and cancellation, verified bytes and bounded disk usage. Respect
explicit offline/skip/managed-install policy. Upgrading an existing opt-out or
custom setup must not trigger a surprise model replacement. No actual model
has been selected or downloaded by this roadmap update. A missing/corrupt model
means "interpreter unavailable," not failed registration or an uncontrollable
node. Residency is configurable and subject to memory limits, not assumed free.

Preserve the original user input and attach a validated interpretation containing
input type, intent, goal, context hints, constraints, suggested capabilities,
semantic complexity, one-shot/continuous intent, and optional plan/method hints.
Treat all model output as untrusted: bound bytes/collections/time, validate
schema, reject invented identifiers/permissions, keep unknown values unknown,
and require clarification for ambiguous consequential actions. EEF revalidates
against live state; hints never override authenticated origin or owner policy.
Neither natural language nor an LM-generated plan is executable shell code.
Do not present the small classifier/interpreter as a heavy reasoning model.

## 6-7. Plans, grants and deterministic execution

The unreleased origin planning increment lets the input node request EEF's existing
compiler and inspect its private concrete plan through shared commands, without
using EEF's owner interface or a dashboard. Candidate filtering precedes ranking
and admits only fresh self/owner-disclosed peers; default remains self-only.
Compilation retains `awaiting_approval`, never implicit approval or execution.
Explicit `requests submit`/`jobs request-text` now admit immutable caller input
directly into that reviewed compiler path, without interpretation or inference.
Failed compilation may leave a source checkpoint; exact receipts can inspect or
explicitly finish eligible partial admission without replacing its original input.
This is a grant-aware entry point, not migration of legacy task/chat/rule/RPC
execution or completion of the grant-enforced default; see [explicit requests](EXPLICIT-REQUESTS.md).
Simple parameter-free inputs have a file-free CLI adapter; explicit `requests
prepare` can now produce a source-bound one-step method or answerable clarification
using the selected interpreter. EEF atomically checkpoints it against immutable
review/context; a method still awaits separate approval. Historical snapshots and
saved preparation receipts do not renew lookup/authority or require inference.
Bounded input/answer triggers now share runtime-owned services. General semantic
planning, durable automation and delegated owner approval are not claimed; see
[origin planning commands](COMMANDS.md#planning-through-the-input-node-unreleased-after-alpha6).

The latest unreleased owner approval increment saves `approved` against the exact
compiled digest/revision, original context, issuer fingerprint and explicit
expiry/execution/result limits. It rechecks fresh fixed providers and grant
support, with no model call, download, reservation or work dispatch. Exact receipt
retries inspect without renewal; restart, expiry and origin-owned cancellation do
not revive execution. Legacy runners reject these records. This completes an
explicit conditional approval slice. The subsequent execution increment durably
issues exact fixed grants and directly checkpoints matched ACK/completion in the
same job journal. Original node input can select a different approved executor;
only owner/origin can activate the retained request. Current admission reserves
provider/node/global future budgets for granted model and native work, not physical
memory or model-instance budgets. Uncertainty after restart never auto-replays,
even for read-only work. Automatic semantic planning, reconciliation and the final
non-coder request-to-execution workflow remain open; see
[commands and limits](COMMANDS.md#bounded-owner-request-approval-unreleased-after-alpha6).

The next read-only recovery increment adds origin/owner `requests receipt`, using
the exact retained issuance and the executor's same pinned issuer. Scope digests
and strict identity/timestamp metadata match; reserved controls remain available
while workload slots are full. It works across session replacement without
reviving old authority, changing the job, inferring success or authorizing replay.
This supplies recovery evidence, **not** automatic or owner-directed uncertainty
resolution, lost-output recovery or proof that external effects stopped.

The subsequent [retained-results increment](RETAINED-RESULTS.md) atomically saves
bounded terminal reports/output in the executor's existing grant journal before
completion delivery. Admission reserves that space across independent handles;
legacy migration preserves consumed history without inventing success/output.
Origin/owner `requests result` and executor-local `grants result` explicitly fetch
private evidence without changing EEF tracking, renewing authority or repeating
effects, including after session replacement. Receipt/status paths stay redacted.
This advances lost-output retrieval, not safe replay, explicit coordinator journal
reconciliation, strong termination or physical release acceptance.

The explicit `requests abandon` recovery branch now permits owner/origin closure
of idle interrupted tracking, with exact revision/grant, an acknowledged effects-
may-continue flag and durable decision receipt. It preserves the unknown issued
attempt and original context under distinct `abandoned`, never success/remote Stop.
Live runners and closing admission deny; exact same-actor retries inspect; restart,
legacy runners and fresh execution commands cannot revive it. Optional history
removal is separate and does not erase node nonce protection. This is one explicit
recovery decision, not lost-output recovery, safe replay/migration or strong
executor termination. Those remaining stage 6-7 boundaries still apply.

The credential-only startup follow-up removes the unused legacy PSK requirement
when legacy authentication is explicitly disabled. No legacy key is constructed
or advertised; independent TLS identity/credential checks still apply, and owner
re-enablement requires a valid distinct legacy secret. Cold restart and two logical
node reconnects pass without a config/env legacy secret. This improves setup
reliability, not automatic pairing, credential ESP support or physical acceptance.

The 2026-10-10 unreleased [scheduling command increment](COMMANDS.md#coordinator-scheduling-limits-unreleased-after-alpha6)
adds owner-configured non-model freshness and coordinator-local global/provider/
exact-node concurrency ceilings. Atomic reservations include retired futures
in global/node budgets until release; saving policy requires explicit EEF restart.
This is stage 7 admission infrastructure, not physical resource enforcement,
legacy model inference admission, remote cancellation acknowledgement or a grant.
Direct owner diagnostics/recovery remain usable when scheduling capacity is full.

The 2026-10-10 unreleased [independent credential increment](NODE-CREDENTIALS.md)
adds exact-node provisioning, pairing, rotation, revoked-ID tombstones and an
explicit legacy migration switch. Credential changes apply after coordinator
restart. The subsequent TLS increment puts credential framing inside standard,
owner-certificate-pinned TLS 1.3, with no downgrade, early data or resumption.
TLS record replay protection does not implement durable logical-command replay
state or scoped issuer-session authorization; those remain required before safe
execution grants/peer paths. This is a stage 6 prerequisite, not stage 6 completion.

EEF uses the structured goal plus actual capabilities, models, freshness/load,
reachability, priority, ownership and resource limits to build the authoritative
plan. The LM may describe semantic complexity, not assert exact compute cost.
Preserve the current scheduler and extend its constraints and measured inputs.

The unreleased attempt-assignment increment now persists at most one executor
per durable task attempt. Model assignments retain node/backend/model identity;
older missing backends remain unknown. Durable model jobs no longer traverse
providers inside one attempt; any existing policy-approved retry advances the
attempt checkpoint. Duplicate/stale assignments cannot rewrite the journal or
mark an unattempted provider unavailable. This is a local stage 6 journal
prerequisite, not node-side grant/replay enforcement or acknowledged migration.

An execution grant binds the existing job/attempt identity, issuer/session
generation, executor node, exact capabilities/resources/model or pipeline,
priority, enforceable limits, reporting policy and expiry/termination conditions.
Persist issuance before dispatch, correlate acceptance/completion to that grant,
and reject duplicate/stale attempts. Effective permission is the intersection
of EEF's grant and current local node policy. Revoking local permission takes
effect without waiting for a model or a coordinator reply.

Design grants so nodes cannot forge coordinator authority using a network-wide
shared secret: use separately bound credentials/authenticated issuer sessions
and reviewed, scoped authorization. Do not improvise a new crypto construction.
Transport reuse does not mean reusing a shared PSK as a peer/lease trust root.

Pause/stop/cancel/reprioritize need executor acknowledgements and explicit safe
boundaries, not only updates to a database status. Already-completed side effects
cannot be undone. Revocation on a partition is bounded by the granted duration:
no immediate remote-stop claim while disconnected. Use monotonic execution
deadlines; reject unsafe clock/expiry conditions. Check limits within local loops
and terminate bounded child work where supported; mark unsupported guarantees.

The unreleased fresh issuer lease challenge now removes cross-PC wall-clock
comparison from grant admission/execution. Its exact scope and live-session proof
cannot renew authority; mint-time journaling, whole round-trip time and durable
claim/ACK consume one monotonic budget. Missing-mode nodes cannot receive new
grants; no fallback or implicit update is permitted. Local clock discontinuity
checks remain, and the timer margin is conservative engineering, not hardware
attestation or a hard real-time bound. See [timing contract](EXECUTION-GRANTS.md#limits-and-honest-termination).
Physical two-PC acceptance, strong termination and cluster fencing remain open.

After disconnect, permit only the already-authorized work until its bounded
expiry/policy requires stopping. After restart, do not revive old leases or
replay uncertain mutations. Reconcile persisted attempts and explicit owner
decisions using the existing recovery rules. Lost acknowledgement is not proof
that work never ran. Do not migrate active work to another executor before its
old authority is known to be stopped/expired; clustering remains a later gate.

## 8-9. Peer paths, continuous work and listening

The subsequent [local pipeline structural foundation](LOCAL-PIPELINES.md) adds
strict finite graph/data-reference types sharing existing operation-shape checks.
The execution core adds a parent grant shape, serial local dataflow, existing
deadline/policy watching, retained data accounting and runtime-local progress.
The following command/compiler integration adds explicit finite graph submission,
whole-node eligibility/ranking, fixed stage identities and repeated approval/
dispatch checks. Durable progress/stop acknowledgements, continuous support,
physical resource/media/network and release acceptance are not complete.
Continue those on the existing
durable runtime; the full finite/continuous/media/peer scope below is unchanged.

Owner clarification, 2026-09-22: see [presence and supervised media sessions](PRESENCE-AND-MEDIA.md).
EEF approves and supervises sessions; payloads may use approved peer paths or a
user-selected bounded coordinator relay. This replaces the intermediate proposal
to require every packet to traverse EEF, not EEF's authority over the session.
Persistent virtual audio input/output, configurable microphone/TTS mixing,
independent output routing and reusable lightweight presence are planned extensions
of the existing resource/job/grant model. Presence supports welcoming, locating
and optional handoff; it grants no new permission or assumed person identity.
Account for concurrent sessions and aggregate resource/bandwidth limits. These
were recorded as documentation-only requirements, not implemented features;
existing implementation order and security prerequisites remain.

EEF gives only authorized endpoints and short-lived pair/job/resource-scoped
session credentials. Both peers authenticate each other and validate direction,
scope, generation and expiry; no authorization follows merely from learning an
IP or sharing an overlay. Encrypt payloads, reject replay, bound frames/queues,
connections, transfer bytes/rate and duration. Keep control traffic responsive
under bulk load. Report transfer status/results to EEF. Failed reachability
reports a clear failure or uses an explicitly allowed bounded relay, not an
unbounded fallback or public unauthenticated listener.

Extend durable jobs with local pipelines: camera -> detector -> optional VLM ->
event; microphone -> VAD -> optional STT -> interpretation/filter -> event.
Do not send every normal frame to EEF. Set rate/resolution, memory/disk/CPU
budgets, event deduplication, backpressure, bounded offline buffering and privacy
retention before claiming continuous support. Add recurring triggers to the
same job/attempt system with explicit overlap, missed-trigger and restart rules.

The listening toggle means ambient input on explicitly selected nodes, **not**
EEF service uptime. Default ambient capture is off and requires the local
microphone permission plus an authorized listening workload. EEF may request it
through normal approved commands but cannot override local denial. Listening off
stops ambient capture/processing and releases resources; typed commands, network,
job control and other authorized work remain available. Status must expose which
node is listening. Heavy LLM/VLM residency is never required to remain reachable.

## 10. Model lifecycle

Track UNLOADED, LOADING, READY, ACTIVE, WARM_IDLE and ERROR from backend/process
observations, not desired configuration. Unknown backend state remains unknown.
Bound concurrent loads, report load failure/cancellation, and pin models while
active workloads use them. Unload EEFN-owned heavy processes after configurable
idle time; do not stop a shared Ollama service or disrupt another user's active
work. Backend limitations must be explicit. Lightweight residency is optional.

Schedule using measured cold-start cost, available memory, current load and
expected workload duration. A warm slower node may win a short request while a
cold faster node may win sustained work; these are test scenarios, not hardcoded
node preferences. Enforce budgets even when estimates are absent or inaccurate.

## Acceptance and remaining scope

Every slice needs unit/protocol tests, old-config/wire migration tests and
command-driven integration without a browser or an available interpreter.
Keep baseline authentication, origin, resource privacy, permission, disk,
updater/installers and no-unsafe-replay tests. Keep existing UI smoke tests but
do not expand visual polish work. Test public commands rather than relying only
on test helpers directly accessing internal modules.

Physical multi-node gate: user command/NL on Node A -> interpretation -> EEF
validation/plan/grant -> Node B load/local execution -> status/result -> Node A.
Record actual node IDs and remote execution evidence, network path, model/runtime
versions, permission decisions and measured payload/event counts. Also verify a
continuous pipeline keeps repeated payloads local while EEF receives bounded
useful events. One-PC fixtures and mocked inference remain separately labelled.

Negative gates: wrong/expired/replayed grants, unauthorized peer lookup/session,
permission revocation, malformed/hallucinated interpretation, LM/backend crash,
disk/memory limits, disconnect/partition, stale coordinator hints, cancelled
loads, duplicate submissions and safe job reconciliation after both-role restart.
The owner will be needed for the physical second-PC/real-media acceptance; do not
claim localhost tests satisfy it.

Wi-Fi/Bluetooth capabilities, ESP32/ESP-NOW/multi-hop, sophisticated distributed
optimization/migration and quorum-based coordinator replication/election/fencing
remain later work. Preserve them in the backlog; advertisements and endpoint
priority do not implement safe active/standby. Defender/signing investigation
remains a release gate, not UI work. Do not change published artifacts, stable
feeds or contributor/account isolation as part of this roadmap revision.

## Development checkpoint: 2026-09-11

- Extracted runtime-owned `NodeService` from dashboard state/config/remote
  management. Model manager and engine use the service; HTTP is an adapter.
  `NodeDashboard` remains a compatibility alias. Not every command family has
  been extracted/exposed yet.
- Added [network commands](COMMANDS.md), offline instance-lock protection,
  running-node commands, JSON output, and `--no-ui` API-only operation.
- Validated independent advertisements and optional coordinator hints pass
  through registration to EEF's existing node view. Old configs need no rewrite;
  old node IDs remain unchanged. Saved/applied addresses differ until restart.
- 91 locked Rust tests passed. Public-command integration passed in
  `.validation/node-commands-NNknMl/`; existing browser mocks and real-process
  first-run regression passed in `.validation/first-run-PSCYYk/`. No physical
  second-PC, peer-session, default-LM or lease claim follows from these tests.
- No release, model download, installer replacement, signing or auth change.

Next: more shared commands, general model metadata and the remaining ordered
increments. Published v0.4.0a stays unchanged.

### Scoped discovery follow-up

- Added `eefn network peers` through existing authenticated submission, using
  immutable origin and exact coordinator-owned disclosure grants. Default is
  self-only, without symmetric/transitive grants or automatic coordinator trust.
- Sanitized live-connection records exclude private bindings and observed source
  endpoints. Monotonic heartbeat age removes stale addresses/model summaries;
  disconnect removes the record. Pages and policy/registration sizes are bounded.
- Grants configure disclosure, never direct execution. Policy/revocation applies
  on coordinator restart. The subsequent command checkpoint below adds owner controls.
- 97 locked Rust tests passed; command integration passed in
  `.validation/peer-discovery-lXlWL2/` and `.validation/node-commands-VEVMDV/`.
  The former runs two logical nodes on one PC, not a physical two-PC test.
- Existing first-run/browser regression passed in `.validation/first-run-IbZSxV/`;
  dashboard initialization/copy-stability mocks also passed. No visual UI work.
- Authentication still inherits the shared-PSK trust boundary; exact-ID grants
  do not isolate mutually untrusted key holders. Independent peer/issuer
  credentials remain required before direct paths or execution leases.

### Owner commands and real-network checkpoint

Unreleased follow-up adds `eef discovery show/grant/revoke`, `eef restart` and
owner-approved `eef node restart`. Configuration edits use the latest saved state
under a shared lock, preserve unrelated settings, and expose saved/applied policy.
Exact-ID disclosure still needs EEF restart to apply. Node restart reports request,
acknowledgement and observed completion separately and does not replay uncertainty.
Outbound queue waits are bounded; cancelled waiters release pending correlations.

109 Rust tests passed. Live testing on the owner's two PCs confirmed registration,
temporary one-way discovery followed by revocation, automatic reconnection across
two EEF restarts, and authenticated remote ping samples. The real node denied
restart without its local management approval; approved restart was validated in
isolated real-process tests at that checkpoint. After the owner enabled the local
management switch, `.validation/live-discovery-QGI8x0` also confirmed actual
second-PC restart with stable identity and a new runtime ID. No user permission
was enabled remotely. The published coordinator executable was restored after the
live development test. General model metadata remains the next ordered increment;
full workloads, direct peer transfers and independent credentials are still pending.

### Read-only model inventory foundation

The next development slice adds `eef models list`: a bounded owner view of
current registered models with node/backend/model identity, legacy capability
and input/output modality normalization, filters, node pagination and freshness.
Unknown roles, memory estimates and lifecycle remain unknown; model names do not
infer capabilities. This does not change config, backend discovery, loading or
scheduling, and does not complete step 3's general multi-capability contract.
No model is downloaded or loaded by the command. See [command scope](COMMANDS.md).

### Versioned model registration increment

The subsequent increment adds a shared bounded metadata contract to existing
registration, sanitized discovery, inventory and model routing. New Ollama/GGUF
nodes emit it alongside legacy fields. Explicit capabilities override legacy
labels; unknown metadata stays unknown. Node model snapshots replace atomically,
and backend failure state is isolated. See [contract and remaining scope](MODEL-METADATA.md).
General owner configuration for non-text/vision models, role-based scheduling,
default LM bootstrap, leases and lifecycle controllers are not yet implemented.

## Installation direction: MSI

The owner requested MSI on 2026-09-11. Add separate EEF and EEFN MSI packages in
a packaging increment, retaining current installers until replacements pass
acceptance. Reuse verified payloads and inventories rather than a second build
of the application. MSI tooling has not been installed; no MSI was built here.

Acceptance: per-user dedicated-directory setup where supported, explicit startup
choice, stable product/upgrade identity, repair/uninstall, preserved config/models/
job data, locked-file/restart handling, old EXE-install migration, unattended
installation and truthful errors. Do not automatically delete user data on
uninstall. Test both roles together and upgrades on a second PC. Signing, hashes
and Defender checks still apply; MSI is not an antivirus or UAC bypass. Core
command work remains the immediate priority, not installer UI polish.
