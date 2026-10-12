# Reviewed client acceptance (unreleased after alpha.6)

This checkpoint adapts remaining protocol/browser/model fixtures to the mandatory
[reviewed boundary](EXECUTION-BOUNDARY.md). Both production roles remain at the
unreleased development state after alpha.6. No published installer, update feed,
installed runtime or physical node is changed by this checkpoint.

The initial 2026-10-11 checkpoint passes 390 workspace/all-target Rust tests both
with default dashboard features and with `--no-default-features`. Fresh default
executables pass the metadata fixture in API-only and saved-UI-preference modes,
first-run/browser, reviewed requests and verified-artifact real CPU inference.
Formatting, JavaScript syntax, the backend/browser dependency guard and diff
checks pass separately. No model download, legacy-node compatibility run,
physical update, installer rebuild or security-scanner certification is claimed.

## Contracts checked

- Model metadata/management: explicit backend and role selection, capability
  restrictions, saved/applied separation, authorization revocation before a pull,
  quota rejection without mutation, reserved ping/status/restart while work is
  busy, restart without replay, pending API port discovery and empty snapshots.
  Public source/compiler/owner-approval/execution commands replace inference RPC.
- Model replies: completed, partial and older finish metadata remain distinct.
  Invalid/missing/oversized/chunked responses and body timeout return actual
  unknown evidence, not a fabricated successful task. Disposable interrupted
  sources require explicit abandonment acknowledging effects may continue.
- Creation receipts: duplicate exact input reuses the retained source without
  inference; conflicting input fails. A lost local HTTP reply is found by its
  pre-send receipt and remains awaiting approval until explicitly executed.
- Browser/CLI: automatic local registration and stable ID, approval-gated remote
  configuration/restart, permission forms, private selected resource binding,
  authenticated immutable origin/area, persisted jobs/drafts, copy/selection/focus
  retention, mobile layout and storage limits after a verified EEF restart.
  Forged origin on browser input is rejected. Missing interpreter/role cannot
  invoke a keyword fallback. With the subsequent input-workflow policy, fresh
  browser/CLI input prepares an eligible plan awaiting approval, not a workload.
- Storage: retained review drafts reserve future checkpoint space. Explicit
  cancellation/removal releases that accounted space; reviews are not treated
  as finished jobs merely because they have no running executor.
- Real model: opt-in CPU inference uses exact reviewed grants, then a verified
  node restart, unchanged completed work and retained output across the new
  session. Model withdrawal is checked independently. Existing artifacts must
  match exact catalog size/hash; reuse does not copy/download model weights.

The test-only shared public-command adapter never emits raw workload RPC, edits
production journals, retries a mutation, changes policy or synthesizes results.
Unsupported pause/resume is refused unchanged. Stop waits for returned backend
evidence; these checks do not prove native/backend termination.

## Diagnostics and onboarding limits

`last_workload` records scope only after durable grant admission and a successful
ACK write, just before execution. Controls cannot erase this record; rejected
work does not replace it. It excludes private parameters/output. Its
`execution_may_have_started` flag is deliberately not proof of completion,
all current effects, latency or remote stop. The snapshot is live; durable
grant receipts/results remain authoritative for retained evidence.

Interpreter setup errors provide a backend-authored `setup_hint`, including
selection of exactly one `request_interpreter` model and applying changes with
restart. CLI and HTTP input use the same guidance; no role/model is chosen
automatically and the dashboard does not decide policy.

The browser fixture explicitly provisions pinned credentials and configures
the interpreter role with owner commands. The existing model picker assigns
no interpreter role. It therefore reports `non_coder_setup_complete: false`:
passing forms/commands does not satisfy first-time installation through approved
work without technical setup. Its browser input field is currently single-line;
leading/trailing spaces are checked there, multiline text separately via CLI/HTTP.
Cross-node origin isolation is checked in the two-logical-node request fixture,
not invented by creating a retired coordinator-origin workload.

## Running the checks

Use fresh default-feature executables for the browser fixture. The metadata
fixture also supports no-dashboard executables; `EEF_TEST_UI_CONFIG=1` requires
default-feature executables to test saved UI visibility separately from the API.

```powershell
node tools/test-model-metadata.mjs
node tools/test-first-run.mjs
$env:EEF_TEST_BINARY_DIR = 'ABSOLUTE_FRESH_BINARY_DIRECTORY'
$env:EEF_TEST_MODEL_FILE = 'ABSOLUTE_EXISTING_CATALOG_MODEL_FILE'
node tools/test-real-model.mjs ABSOLUTE_RUNTIME_BUNDLE_DIRECTORY
```

The runtime bundle supplies llama.cpp; `EEF_TEST_BINARY_DIR` can point at freshly
built roles instead of old packaged executables. Choose a catalog model with
`EEF_TEST_MODEL_ID` if needed. Without an existing model, a download requires the
additional explicit `EEF_TEST_ALLOW_MODEL_DOWNLOAD=1` opt-in and occurs only in
the disposable fixture. Never select or alter a personal model for this test.

The subsequent [input workflow acceptance](INPUT-WORKFLOW.md) adds seven Rust
tests (397 total in both feature modes) and an API-only public-command fixture.
Fresh browser/CLI preparation and retained pending plans replace review-only input
assertions; explicit review-only policy remains tested in the request fixture.
The new checks do not claim automatic approval or workload execution.

Fresh executables pass input workflow (`request-input-CqhxNt`), request regression
(`request-review-1leGwq`), browser/first-run (`first-run-dS5DGq`), metadata API-only
(`model-metadata-Il9vqT`) and saved-UI
(`model-metadata-WPWhkN`) checks, plus verified-artifact real CPU inference
(`real-model-wybvNt`). These local fixture directories are evidence, not release
artifacts or public model defaults. No model download or physical update occurred.

These checks do not prove interpreter quality/default model suitability, physical
two-PC acceptance, continuous media pipelines, peer transport, cluster safety,
strong termination, MSI onboarding or Defender/Authenticode acceptance. All
remain on the full [active roadmap](ROADMAP-CORE.md); no stable release claim.

## Answer workflow checkpoint (2026-10-11)

The [explicit-target answer workflow](ANSWER-WORKFLOW.md) adds six Rust tests
(403 total, passing with default features and `--no-default-features`). Fresh
executables pass the expanded input/answer fixture (`request-input-7MhPvK`),
request regression (`request-review-xzZMAI`), first-run/browser
(`first-run-P4HtCg`), metadata API-only (`model-metadata-BrmNY5`) and saved-UI
(`model-metadata-dZU8ZS`) fixtures, and verified-artifact real CPU inference
(`real-model-TlamQS`). An earlier request-regression invocation exited without
diagnostics and was not counted as a pass; the rerun adds failure logging.

The answer fixture checks two clarification rounds, exact source digests and
history, independent saved/applied policies, busy admission, runtime replacement,
concurrent edits, unknown/lost replies and origin isolation. Reused receipts only
inspect history, including after policy changes and interpreter withdrawal.
Prepared methods remain pending approval; no workload is admitted by this workflow.
These are local logical-node fixtures, not physical two-PC or default interpreter
quality acceptance. No models were downloaded/copied, installed runtimes changed,
release artifacts replaced or update feeds modified. All release gates above remain.

## Explicit interpreter setup checkpoint (2026-10-11)

The [atomic interpreter role commands](INTERPRETER-SETUP.md) add seven Rust
tests (410 total in both feature modes). The full metadata fixtures pass in
API-only (`model-metadata-U31v6L`) and saved-UI (`model-metadata-igRWPB`) modes.
They cover approved/denied coordinator commands, role preservation, no-op/clear,
saved/applied separation, explicit restart, lost/substituted replies after real
configuration saves and old-endpoint refusal. No model inference or download
is caused by assigning/clearing a role.

The input/answer fixture passes (`request-input-h8zkEM`) with interpreter
withdrawal through the new command and read-only retained history after restart.
First-run/browser regression passes (`first-run-ImtFAE`); this fixture still
uses explicit owner pairing and is not complete non-coder setup acceptance.
The full request regression also passes (`request-review-x56Izd`).
After the final command-report change, fresh binaries pass metadata API-only
again (`model-metadata-whMtGJ`).
Exact saved role validation is not model quality or applied runtime readiness.
No UI assets, personal installations, physical nodes or release feeds changed.

## Interpreter formatting and quality checkpoint (2026-10-11)

The bounded generation schema adds one Rust test (411 total, passing in both
feature modes). Fresh binaries produce passing input/answer
(`request-input-VUJa5K`), full request regression (`request-review-Ixxtce`) and
real CPU reviewed inference (`real-model-Rsq7pb`) fixture reports. The expanded
input fixture verifies backend format refusal never retries unconstrained,
falls back, or saves an opening receipt. Independent validation still rejects
invalid model replies; method preparation and ordinary inference are unchanged.

The [production quality harness](INTERPRETER-QUALITY.md) records a separate
20-case semantic evaluation using the existing verified artifact, no coordinator
and no grants. Ten proposals are accepted but only three match the fixed rubric.
This is not default-model approval or physical acceptance. All full-roadmap
release gates remain open; no UI assets, installed nodes or release feeds change.

## File-free private pairing checkpoint (2026-10-11)

The [shared private-input command](NODE-CREDENTIALS.md#file-free-invitation-input-unreleased-after-alpha6)
adds four core tests and one CLI parser test (416 total, passing with default
features and `--no-default-features`). The old private-file adapter shares the
bounded decoder/validator; it is not removed. Fresh default binaries pass the
new public CLI/API fixture (`private-pairing-W2pMwa`), existing independent
credential/TLS/rotation/revocation fixture (`node-credentials-uCnhaG`), browser/
first-run regression (`first-run-EKcbDd`) and input/answer regression
(`request-input-Ax2sFl`). The source backend/browser boundary also passes.

The new fixture verifies exact target/runtime, body/input bounds, UTF-16 pipe
input, unchanged saved identity/permissions/models and paused preference,
separate explicit coordinator restart/resume, actual pinned TLS registration,
lost/substituted replies after real saves, unexpected private fields/injected
prose, unsafe error bodies, old-endpoint refusal and stalled-input timeout.
No private invitation file or feature approval is created by pairing. Reports
confirm saving/restart request, not completed restart or model/workload readiness.
An added fault-injection unit test initially failed because its cleanup tried to
rename a stable ID through generic settings; cleanup now restores only its own
simulated external corruption. Both full test modes were rerun successfully.

These are isolated one-PC processes, not physical two-PC or complete non-coder
onboarding acceptance. UI assets, personal installations, release artifacts and
update feeds are unchanged. Private same-PC provisioning/orchestration, approved
model bootstrap, durable automation, full runtime/peer/media/cluster work and
MSI/Defender/Authenticode gates remain on the full active roadmap.

## Optional model and production quality checkpoint (2026-10-11)

The manual [1.5B catalog option](MODEL-CANDIDATE-EVALUATION.md#optional-15b-candidate-2026-10-11)
adds one bundled-catalog contract test (417 total). The exact revision, file size,
SHA-256 and declared license are recorded; existing first/default catalog
behavior, owner-managed providers and interpreter role selection are unchanged.
The real-model fixture's opt-in download now uses public preflight/admission,
operation-ID status and installed-inventory commands, checking installation
does not silently select a role. Its one real download/inference run passed
(`real-model-9JKfMN`); later fixtures reuse that verified artifact without copies.

Three [production quality runs](INTERPRETER-QUALITY.md#larger-candidate-and-rejected-prompt-experiments-2026-10-11)
complete on the same twenty-case frozen rubric. Placeholder copying makes the
original prompt's 12/20 label matches misleading; both value-free alternatives
regress labels and are reverted. The harness now records a shared Rust prompt
reference hash. Neither candidate is approved as an automatic default, and
production prompt/validator/approval rules are unchanged.

After reverting the experiments, fresh default binaries pass all 417 Rust tests,
real CPU reviewed inference/restart/retained output (`real-model-VSXd3e`), input/
answer workflow (`request-input-txMUlX`), request regression
(`request-review-AuXEYS`), first-run/browser (`first-run-oi23OO`), model-download
commands (`model-downloads-myOSwP`), the frozen rubric and source backend/browser
guard. An initial no-dashboard suite run timed out waiting for a pinned grant
session while other fixtures ran in parallel; it is not counted as a pass.
That exact test passed separately, then the full no-dashboard suite passed all
417 tests without other fixtures running. Timing contention is a possible cause,
not a proven diagnosis; no timeout or security assertion was loosened.

These are isolated local logical-node tests, not physical second-PC acceptance,
bootstrap or release acceptance. UI assets, installed nodes, public installers
and update feeds remain unchanged. Durable automation, full resource/termination,
peer/media/cluster and MSI/Defender/Authenticode gates remain open.

## Local-model CPU settings checkpoint (2026-10-11)

[Shared owner CPU commands](MODEL-CPU-THREADS.md) add five tests (422 total).
The setting is per existing GGUF selection, bounded, explicit and saved-only;
approved coordinator management uses the same service. Roles/restrictions,
provider, private paths and other configuration survive. Reselection without a
new count preserves the old count; clearing requires an explicit command.
Exact saved-value confirmation rejects substituted replies without replay,
fallback, provider change or restart. Old settings remain backend-default/unknown.
Optional public advertisements carry the configured count, not CPU/RAM estimates.

The launcher configures generation/prompt workers together and now passes zero
GPU layers explicitly for the CPU setting. This is not a total process/CPU/RAM
quota, physical resource measurement, native termination proof or default-model
approval. Ordinary reviewed grant/configuration fencing is retained.

Both full workspace/all-target suites pass all 422 tests after the final change.
Fresh binaries pass command metadata with saved UI preferences and CPU fault
injection (`model-metadata-ryecSb`), first-run/browser (`first-run-CZZsJm`),
verified-artifact actual CPU inference (`real-model-H7NFNK`) and the source
backend/browser guard. The real-model fixture now uses public selection/removal
commands rather than editing model configuration JSON, checks saved/applied
counts and coordinator inventory, and checks its owned Windows model process
arguments before reviewed inference/restart/result-retention/withdrawal.
The existing artifact is reused, without new downloads or copies.

One earlier saved-UI fixture (`model-metadata-1ZiOMo`) failed on a truthful
`node_busy`/`not_started` reply during repeated remote hints at work capacity one.
It is not counted as passing. The standalone rerun (`model-metadata-zS461C`) and
final fresh-binary run pass; admission timing remains a reliability follow-up,
not a diagnosed/fixed race. No hidden retry or relaxed capacity assertion was
introduced. UI assets, installed nodes, installers and feeds are unchanged.
Physical, bootstrap/quality, durable automation, full runtime/peer/media/cluster
and MSI/Defender/Authenticode gates remain open; stable 0.4.0 is not claimed.

## Bounded node output checkpoint (2026-10-11)

[Session output](TRANSPORT-OUTPUT.md) adds six Rust tests (428 total). Both full
workspace/all-target feature-mode suites pass. The real TCP fault unit reproduces
the earlier unbounded ordinary-reply behavior before the fix and passes afterward.
Fresh default executables pass the process-level output fault fixture
(`node-output-2kR8eg`): connection retirement/reconnection in 6,332 ms, unchanged
node runtime and saved settings, usable local commands, no busy rejection and no
old response/work traffic on the new connection. An invalid-config precursor
(`node-output-YmpDBE`) is not counted as passing; see the linked evidence/limits.

Public regressions also pass: node commands (`node-commands-JCeQrQ`), input
workflow (`request-input-RwzADu`), reviewed requests (`request-review-b5R0Ew`),
private pairing (`private-pairing-k49buW`), separately authenticated logical nodes
(`node-credentials-lfG2Tp`), model download commands (`model-downloads-rsCaoc`),
model metadata/API-only (`model-metadata-sSqXRT`), saved UI preferences with CPU
fault cases (`model-metadata-TDBFmy`), first-run/browser (`first-run-Or9zys`) and
the backend/browser source guard. Formatting, JavaScript syntax, a fresh default
binary/example build and diff checks pass separately.

Actual pinned-TLS CPU inference (`real-model-DcfZd4`) reuses the exact existing
1.5B artifact, checks explicit generation/prompt CPU settings and zero GPU layers,
executes reviewed grants, verifies restart/retained results and withdraws the
selection. No weights are copied or downloaded. A successful greeting does not
approve this model's interpretation quality or make it a bootstrap default.

The fix does not diagnose the prior intermittent busy response, prove native
termination, complete durable automation or replace physical acceptance. Published
versions, installers, installed nodes, update feeds and UI assets are unchanged.
Full stable 0.4.0 acceptance remains open.

## Durable automation authoring checkpoint (2026-10-12)

[Source-bound definitions](AUTOMATION-DEFINITIONS.md) add thirteen tests, for
441 total. Both final full workspace/all-target feature-mode suites and the fresh
default executable/example build pass. The public CLI/API authoring fixture
(`automation-definitions-wHI93Z`) uses two pinned-TLS logical nodes, exact source
submission/retention, scope refusals, lost-reply inspection, pagination, concurrent
deduplication, coordinator restart and compact removal receipts. Definitions
remain disabled; no timer, reusable approval, occurrence or execution is claimed.

Fresh public regressions pass node commands (`node-commands-j7CKie`), output
retirement/reconnection (`node-output-i99KyY`), model metadata with saved UI
preferences (`model-metadata-7ZVwUU`), first-run/browser (`first-run-xGsFNN`) and
verified-artifact pinned-TLS CPU inference (`real-model-havd35`). The latter
reuses the existing exact 1.5B artifact without copying/downloading weights;
it checks reviewed grants, CPU launch settings, verified restart, retained output
and model withdrawal, not semantic-model quality or total resource quotas.
The backend/browser source guard, JavaScript syntax and diff checks pass separately.

An input/answer fixture (`request-input-6Rt7dR`) failed during its second
clarification round with `preparation_not_confirmed` while other fixtures ran.
It is not counted as passing. Read-only inspection of its retained journal shows
revision 6, four proposals, two answers, no compiled plan and no execution record.
The standalone run (`request-input-NVkYUB`) passes unchanged production behavior.
The separate reviewed-request regression (`request-review-L2J0xl`) also passes.
Cause is unproven; no hidden retry, weakened assertion, policy change or claimed
busy-race fix is introduced. The fixture now retains unexpected CLI replies in
its isolated directory so a future failure preserves the nested evidence.

Installed nodes, published versions/installers, update feeds and browser assets
are unchanged. Durable scheduling/reusable authorization, bootstrap quality,
physical acceptance, complete runtime/peer/media/cluster behavior and
MSI/Defender/Authenticode release gates remain open.

## Checkpoint diagnosis and owner automation commands (2026-10-12)

Four new tests bring the full workspace/all-target total to 445, passing both
default and no-default-feature modes. The subsequent fresh default executable
and example build passes. Formatting, JavaScript syntax, diff checks and the
backend/browser source boundary also pass.

Targeted recovery of the original fixture's full saved tool output shows
`preparation_unconfirmed`, with inference requested, saving attempted and
uncertainty true. It does not establish whether the coordinator rejected,
transport failed or success confirmation failed. Three unchanged concurrent
fixtures (`request-input-bIhk3P`, `request-input-NUn14n`,
`request-input-YYWtLP`) passed before the diagnostic increment; those passes are
not treated as a fix for the original intermittent failure.

[Checkpoint diagnostics](SEMANTIC-PREPARATION.md) now preserve bounded private
text on authenticated rejection, separately from transport/invalid-success
uncertainty. Rejection still does not prove no commit. The two concurrent public
input/answer fixtures (`request-input-R7O0LS`, `request-input-BklB8J`) pass with
the new deterministic source-edit race: a stale method checkpoint is rejected,
newer history remains intact, and repeating the original answer receipt only
inspects history without inference or execution. Unexpected CLI failures now
retain bounded coordinator/node runtime logs, not credential-provisioning output.

The stronger authoring fixture (`automation-definitions-W3oFmF`) passes with the
new `eef automations` local-owner adapter, two pinned-TLS logical nodes and no
UI/models. It verifies owner authoring/show/list/removal, immutable source origin,
foreign node refusals, removed-receipt reuse and all previous durable-definition
checks. Actual owner CLI calls through a fault proxy cover a dropped reply,
substituted source context, negative response after commit, stale runtime and
invalid-runtime preflight. Invalid preflight sends no definition command;
uncertain mutation replies retain exact receipts, and explicit inspection/reuse
finds only one disabled definition. No timer, reusable approval, scheduled
occurrence or execution is claimed.

Fresh final-build regressions also pass the input/answer workflow
(`request-input-94891w`) and the broader reviewed execution/recovery workflow
(`request-review-KFnYa3`). The latter remains isolated fake-backend/pinned-TLS
evidence, not physical two-PC, native termination or real-model quality evidence.

The prior intermittent input/busy failures remain open investigation items.
Installed nodes, browser assets, releases/installers and update feeds remain
unchanged. The full stable-0.4.0 requirements and release gates remain in scope.

## Reusable owner-approval record checkpoint (2026-10-12)

The [bounded reusable approval record stage](AUTOMATION-APPROVALS.md) adds twelve
tests, bringing both passing full workspace/all-target feature-mode suites to
457. The fresh default executable/example build, formatting, diff/JavaScript
checks and backend/browser source guard pass.

The stronger public authoring fixture (`automation-definitions-B1ofi6`) uses
two separately authenticated pinned-TLS logical nodes and actual owner CLI/API
commands. It verifies fixed scope and generation, foreign node/origin refusals,
policy digest denial at the original job's execution boundary, sixteen-way exact
receipt deduplication, private-scope-free pagination, revocation and historical
inspection after coordinator restart/source-definition removal. Owner policy
fault-proxy cases cover dropped/substituted/negative-after-commit replies and
stale/invalid runtime. Inspection/reuse preserves one exact record, not a new
receipt or renewed authority. The source job remains unchanged and unexecuted.

Journal tests independently cover source/compiler validation after definition
removal, future schema and missing generation history without pruning, shared
job/definition/policy byte and count quotas, and reserved revocation headroom
under lowered quotas/backward clock observation. No occurrence budget is consumed,
job approved, grant issued or timer started by this stage. Reports explicitly
state `occurrence_admission_available: false`.

Fresh final-build input/answer (`request-input-luGLfF`) and reviewed
execution/recovery (`request-review-VEATQU`) regressions also pass. These are
isolated local fixtures, not physical two-PC, approved interpreter quality or
hard native termination evidence. The earlier intermittent input/busy failures
are not diagnosed by successful regressions and remain open.

Atomic new compiled-job delegation, aggregate consumption and every-issuance
policy fences must precede timers. Safe overlap/restart/missed-trigger behavior,
the other full roadmap requirements and installer/Defender/Authenticode gates
remain in scope. Installed nodes, releases, feeds and browser assets are unchanged;
stable 0.4.0 is not claimed.

## Explicit atomic occurrence reservation (2026-10-12)

[Manual owner reservation](AUTOMATION-OCCURRENCES.md) connects reusable policies
to independently compiled matching jobs and the existing grant driver. Twelve
new tests bring both final passing full workspace/all-target feature-mode suites
to 469. The fresh default executable/example build, formatting, JavaScript/diff
checks and backend/browser source boundary pass separately.

Nine journal tests verify new-job approval without source mutation or dispatch,
read-only exact receipt reuse, no overlap/refunds, run/duration/result exhaustion,
time/runtime/issuer/revocation/generation fences, receipt conflicts, atomic
insert-fault rollback, retained restart/future-row refusal, second-write byte
quota rollback, and expiry/backward-clock refusal before commit. Two shared
contract tests and one CLI test enforce exact options, strict nonexecuting
correlation and private-scope-free bounded lists.

The public fixture (`automation-occurrences-4vLRpb`) uses two separately
pinned-TLS logical nodes without models/browser control. Sixteen concurrent
requests save one reservation. Explicit ordinary node execution completes one
native `system.info` grant; original/foreign jobs, overlap, revocation and
old-runtime execution are refused. Revocation/restart cases keep the original
time window open and check the actual coordinator policy-fence reason, not just
any failure. Fault-proxy CLI calls preserve uncertainty after dropped,
substituted or negative-after-commit replies; exact inspection/reuse neither
creates a new receipt nor dispatches work.

The initial public fixture (`automation-occurrences-2iNrvd`) failed a test-side
`null` expectation for fields omitted by the existing serializer. It is not
counted as passing; corrected assertions require absence. No production
behavior, authority bound or retry policy was changed for that correction.

Fresh definitions/policies (`automation-definitions-RgZ8SC`), input/answers
(`request-input-tG6LvM`) and broader reviewed execution/recovery
(`request-review-UoGRUU`) regressions also pass. These remain isolated local
evidence, not physical two-PC, real semantic bootstrap quality, general pipelines,
strong native termination or full measured-resource evidence. Previous
intermittent input/busy failures remain unproven.

At this reservation checkpoint, authority is consumed permanently, including
after unissued cancellation; terminal occurrence jobs could not be removed. Safe
archiving/retention, unresolved partial-work handling and timer lifecycle remain
necessary follow-up work. No timer or automatic job creation is enabled. The
remaining full roadmap and installer/Defender/Authenticode gates stay in scope.
Installed nodes, releases/installers, feeds and browser assets are unchanged.
Stable 0.4.0 is not claimed.

## Explicit terminal occurrence archiving (2026-10-12)

The subsequent archive increment adds eight tests, for **477** passing in
each workspace/all-target feature mode (186 EEF library, 11 owner CLI,
6 installer, 268 EEFN library, 6 node CLI). Its fresh default binaries/examples,
formatting, diff, JavaScript syntax and backend/browser source guard also pass.
The core uses the existing journal/admission gate; it is not a browser controller.

Exact owner archive commands atomically retain validated completed or unissued-
cancelled job evidence and remove the live job. Permanent reservation budgets,
no-overlap proofs and original node/request tombstones remain. Insertion,
deletion, quota and backward-clock failures roll back; stale, nonterminal,
missing, future or unexpectedly reintroduced live evidence fails closed.
Receipt inspection remains read-only after restart/expiry/lowered quotas.
The bounded 2 MiB full snapshot retains private data and may add metadata bytes;
archival is not compaction, byte reclamation, automatic retention or a refund.

Public fixture `automation-occurrences-8i85cy` retains actual native `system.info`
completion/receipt evidence and makes exactly one archive for 16 concurrent
owner calls. It inspects the same proof across restart, archives unissued
cancellation after revocation/runtime replacement, refuses original request
replay/nonterminal removal/foreign-origin calls, and handles dropped/substituted/
negative post-commit archive replies as uncertain. Exact receipt reuse neither
refunds nor reactivates authority. This is local logical-node evidence, not
physical two-PC or model quality. The preceding archive run `automation-occurrences-npmUJZ`
also passed before the last two public refusal assertions were added.

Current-build definitions/policies (`automation-definitions-Al9E8e`), input/
answers (`request-input-sOCMdx`) and broader reviewed execution/recovery
(`request-review-NPiW9N`) regressions pass. The last uses fake inference;
previous intermittent input/busy failures remain unproven. Physical two-PC,
timer/recurrence lifecycle, partial-work resolution, real bootstrap quality,
heavy planning, measured resource limits, media/mesh/cluster and installer/
Defender/Authenticode gates remain in scope. Installed nodes, published
releases/feeds and browser assets are unchanged. Stable 0.4.0 is not claimed.

## Durable trigger ownership foundation (2026-10-12)

Twelve added tests bring final workspace/all-target suites to **489** in each
feature mode (EEF library 197, owner CLI 11, installer 6, EEFN library 269,
node CLI 6). The fresh default binaries/examples, formatting, diff and backend/
browser source guard pass. This extends the same journal and grant checkpoints,
not a dashboard controller or a second workload runner.

Internal schedules own immutable fresh request/reservation/execution claims;
cursor/claim writes and final clock/budget checks are atomic. Stop/restart/
missed-window authority cannot silently reactivate work. Existing reservation
and every-issuance checks enforce optional trigger lineage; direct approval and
substitute receipts cannot bypass it. Actual cancellation/archive evidence is
needed for the next fixed-delay claim, while missing admission remains unknown.
Permanent policy allowances cannot be reset by a replacement arm. Seven record
types share count/byte limits, including 4096-byte schedule lifecycle headroom.

These are journal/compiler/fake-clock checks, not native timer execution. Public
enable/stop/inspect adapters, the bounded runtime timer task, fresh ordinary
request/compile/reserve/dispatch, monotonic suspend/clock detection and partial-
claim recovery remain required before public activation. No timer starts and no
job is created automatically. All remaining roadmap, physical two-PC, bootstrap
quality, native termination, resource/media/mesh/cluster and installer/signing
gates remain in scope. Stable 0.4.0 is not claimed.

Fresh current-build public regressions pass occurrence/native execution/archive
(`automation-occurrences-Lf22nd`), definitions/policies
(`automation-definitions-ggPMGn`), input/answers (`request-input-A8wVkQ`) and
reviewed execution/recovery (`request-review-kii2bf`). Public occurrence use
still has zero schedules/triggers/headroom; this is not native timer evidence.
The review fixture uses fake inference, not real semantic quality. Earlier
intermittent input/busy failures remain unproven. Installed nodes, release/feed
artifacts, browser assets and model files are unchanged.

## Fresh-admission source/launch fencing (2026-10-12)

Ten journal tests bring final workspace/all-target suites to **499** in both
feature modes (207 EEF library, 11 owner CLI, 6 installer, 269 EEFN library,
6 node CLI). Fresh default binaries/examples, formatting, diff and the backend/
browser source guard pass. The internal runtime bridge is compiled against
the existing ordinary explicit compiler, atomic reservation and grant driver;
there is no second workload runner or dashboard controller.

Late source insertion/compilation fails after stop or policy change. A one-shot
launch marker survives unknown pre-admission restart; marked/preexisting receipts
cannot resume work implicitly. Claim-bound failure reporting preserves owner
stop/successor authority. Actual cancelled unapproved/unexecuted source evidence
can resolve partial admission, while missing history stays unknown. Source
retention, no-refund and no-replay boundaries remain.

These tests are journal/compiler/clock evidence, not a native scheduled workload.
Public owner adapters, timer manager/monotonic discontinuity handling, bridge
acceptance and partial-source/no-work recovery remain required. All remaining
full roadmap and physical/installer/signing gates stay open. Stable 0.4.0 is not
claimed; installed nodes, release/feed artifacts, browser assets and models are
unchanged.

Fresh public regressions pass manual occurrence/native execution/archive
(`automation-occurrences-SdlG5T`), definitions/policies
(`automation-definitions-oAk0yu`), input/answers (`request-input-Mqi85z`) and
reviewed execution/recovery (`request-review-xiFubs`). These do not exercise the
new internal bridge or a running timer; the review fixture uses fake inference.
Prior intermittent input/busy failures remain unproven.

## Bounded automatic fixed-input scheduling (2026-10-12)

Five added tests bring final workspace/all-target suites to **504** in both
feature modes (EEF library 211, owner CLI 12, installer 6, EEFN library 269,
node CLI 6). Fresh default binaries/examples, formatting, script syntax, diff
and backend/browser source guard pass. Guarded owner commands now enable/stop/
inspect schedules; a bounded runtime timer calls the existing one-shot bridge,
compiler, reservation and grant driver without a dashboard or second runner.

Public `automation-schedules-yn74OE` runs two pinned-TLS logical nodes on one PC,
without models or a UI controller. One activation under sixteen concurrent calls
produces three fresh native system-information jobs/completions automatically,
with original source unchanged. It refuses budget reset/foreign authority,
stops before due and after one actual run, prevents another run after revocation,
interrupts on restart without re-arm/catch-up, and retains uncertainty under
dropped/substituted/negative post-commit enable replies. Exact receipt inspection
finds one actual occurrence, not a replay. The preceding shorter fixture
`automation-schedules-YdNdmD` also passed before the additional cases.

Fresh manual occurrence/archive (`automation-occurrences-49gsFD`), definitions/
policies (`automation-definitions-6vgTMy`), input/answers (`request-input-oDPwa4`)
and reviewed execution/recovery (`request-review-VDmOHj`) regressions pass. The
first occurrence invocation (`automation-occurrences-5gLDQt`) exited with
`-1073740791` and no normal result; it is not counted as passing. No fixture
processes remained and no matching application event was found in the checked
interval. A fresh source-unchanged rerun does not prove the cause or a fix.
Earlier intermittent input/busy failures remain unproven.

This proves local native fixed-input scheduling, not general semantic/heavy
pipelines, full partial/no-work recovery, measured resource/hard native-stop
behavior, all worker/network/disk/power races, physical two-PC, peer/media/cluster,
default interpreter/non-coder onboarding or installer/Defender/Authenticode
acceptance. Those full roadmap requirements remain open. Installed nodes,
releases/feeds, browser assets and models are unchanged. Stable 0.4.0 is not claimed.

## Proof-bound pre-launch refusal recovery (2026-10-12)

Final workspace/all-target suites pass **512** tests in both feature modes
(EEF library 218, owner CLI 13, installer 6, EEFN library 269, node CLI 6),
with fresh default binaries/examples and formatting/diff/script/backend checks.
Eight added tests cover exact concurrent resolution, transactional rollback,
marked/missing/partial refusal, stale/corrupt proof, restart/lowered quotas,
passive correlated owner reports/CLI references, and independent clock samples
during normal processing pauses.

Native `automation-schedules-g9fgE0` passes the earlier schedule cases plus
actual origin disconnect before launch, a retained backend refusal, explicit
resolution without replay on reconnect, and a separately enabled fresh native
completion. Two logical pinned-TLS nodes run on one PC without models/UI; this
is not physical two-PC or semantic interpreter acceptance.

Earlier extended fixtures `automation-schedules-bbadFt`,
`automation-schedules-A26GJQ` and `automation-schedules-TAM4Be` failed after
replacement admission with false clock-discontinuity interruptions. Diagnostics
showed ordinary journal processing included in the long-gap measurement. The
independent bounded monitor removes that measurement error with unchanged
thresholds and due/expiry/no-replay checks. Failed runs are not passing evidence;
the separate older unexplained process exit and intermittent failures remain
unproven. General partial/issued-work recovery and all full roadmap/release gates
remain open; no installed runtime, release/feed, model or UI change is made.

Fresh regressions pass occurrences/native execution/archive
(`automation-occurrences-r5BOzh`), definitions/policies
(`automation-definitions-VRWStm`), input/answers (`request-input-FQpbRn`) and
reviewed execution/recovery (`request-review-EFdrAP`, fake inference). These
runs preceded the final passive nested-proof confirmation hardening and do not
prove its acceptance; the execution/recovery code is unchanged by that hardening.
One subsequent full-suite run rejected an existing fixture's synthetic archive
timestamp preceding its actual cancellation under parallel load. The fixture
now dates archive/settlement no earlier than the cancellation. Production
validation was not weakened; both final 512-test suites pass afterward.

Final native `automation-schedules-mlHTYT` passes with the final binaries and
adds lost/substituted/negative post-commit resolution replies, one exact retained
resolution and no replay. Previous `automation-schedules-9dV8KM` exited 1 with
no normal result/failure report, retained an unresolved refusal, and left no
fixture processes. No matching application/Defender event was found in the
checked interval. Its cause is unproven; a passing traced rerun is not a fix claim.

## Bounded Windows process lifetime (2026-10-12)

Final workspace/all-target suites pass **519** tests in both feature modes:
EEF library 218, owner CLI 13, installer 6, EEFN library 276, node CLI 6.
Fresh default executables/examples, formatting/diff/script syntax and backend
dependency guard pass. Seven added Windows entries include six real native
lifetime scenarios and a test-only helper. They observe held root/descendant
handles after normal completion, process timeout, execution-future cancellation
and grant expiry, preserve unknown-effect/replay refusal, prove independent-job
isolation and reject pre-resume containment failure without running user code.
See [scope and OS references](PROCESS-LIFETIME.md).

The first focused suite and both earlier full 519-test suites passed before the
final incomplete-thread-snapshot refusal. A subsequent parallel executable link
failed; the full diagnostic was truncated and its cause is not established.
It is not a successful build. A single-job node build succeeded, and the final
single-job both-mode suites plus workspace executables/examples succeeded after
that guard hardening. Generated old test executables were cleaned for low disk
space, but no cause/fix attribution follows from the succeeding link.

This is Windows bounded process lifetime, not universal sandboxing, undo/stop
acknowledgement, all adapters, non-Windows descendants, measured full resources,
physical two-PC or installer/Defender/Authenticode acceptance. No installed node,
release/feed, browser asset or model is changed. Full 0.4.0 scope remains open.

Fresh final-binary public regressions pass automatic native scheduling/refusal
recovery (`automation-schedules-sej5Pb`), manual occurrence/native execution/
archive (`automation-occurrences-CfhF9J`), definitions/policies
(`automation-definitions-eNLFQ6`), input/answers (`request-input-28CuKI`) and
reviewed execution/recovery (`request-review-oqAYTp`, fake inference). They
establish regression coverage for those paths, not public reviewed process-tree
or physical two-PC acceptance. Earlier unexplained exits/link failures remain
unproven; no security/signing claim follows from the passes.

## Public cross-node native process lifetime (2026-10-12)

`process-lifetime-TpFkkR` passes six public reviewed-path scenarios: root
completion with inherited descendant pipes, local timeout, grant deadline,
local saved permission revocation, executor restart and coordinator restart.
Two separately authenticated pinned-TLS logical nodes run on one PC with no
models or dashboard controller. Origin process permission remains disabled;
directional owner disclosure permits the independently configured executor.
Submission and approval start no processes. Each actual grant retains the
exact parameters, immutable origin and fixed executor with one attempt.

A read-only native observer holds both live root/child process handles, verifies
creation times and observes both exit signals without killing by PID or treating
missing PID listings as proof. All non-normal cases still retain persisted
`unknown` effects; exact receipt inspection cannot repeat work or replace native
identities. This is fixture evidence, not a production stop acknowledgement or
safe migration/replay authority. The isolated finite native helper is excluded
from installed bundles. [Details and reproduction](PROCESS-LIFETIME.md#public-reviewed-path-acceptance-subsequent-increment).

Preceding `process-lifetime-1ivWWA` passed the six cases before added exact grant
assertions; `process-lifetime-BHKIlg` passed five and `process-lifetime-1xzKba`
passed three. Initial `process-lifetime-5O7xtV` failed on a diagnostic keep-alive
socket closed during restart and is not passing evidence. Only read-only
diagnostic/file polls tolerate transient observations; no mutation/work is retried.
Final both-mode workspace/all-target suites remain **519**, and the fresh default
workspace binaries/examples plus formatting, diff, JS syntax and backend source
guard pass. Full physical/resource/media/plugin/model/installer/signing and other
0.4.0 roadmap gates remain open; installed nodes/releases/feeds/UI/models are unchanged.

Strengthened public `process-lifetime-pmU8lz` passes against actual
`--no-default-features` binaries for both roles. The live held-handle observer's
monotonic timer requires permission-loss and both restart exits within 2000 ms,
before those grants' 10000 ms deadline, so eventual lease expiry cannot substitute
for lifecycle cleanup. Exact scope/one-attempt/no-replay assertions remain.
Earlier dashboard-free invocation `process-lifetime-EH59gk` exited 1 with no
normal result/failure report and no remaining fixture processes; its cause is
unproven. The subsequent passing run with bounded private exit diagnostics does
not prove a fix. Default binaries are restored afterward; no release/update or
full physical/native-stop/sandbox/resource acceptance is claimed.

Final default-binary `process-lifetime-qNeb1d` passes all six strengthened cases
after restoration; both final workspace feature modes remain **519** tests.
The dashboard-free fixture observed permission-loss exit after 102 ms, executor
restart after 578 ms and coordinator restart after 388 ms from live observer
admission. These are local fixture measurements, not latency guarantees for other
devices, suspended/overloaded systems or different adapters.

## Typed native process authoring and unsuccessful exit handling (2026-10-12)

Final workspace/all-target suites pass **529** tests in both feature modes:
EEF library 219, owner CLI 13, installer 6, EEFN library 283, node CLI 8.
Fresh default binaries/examples, formatting/diff/JS syntax and backend source
guard pass. Six added typed-source/CLI tests preserve raw text, literal/empty/
hyphen arguments, exact receipts and optional choices, enforce existing argument/
wire/timeout bounds and reject authority/template overrides before submission.
Four additional outcome tests retain native failed-exit evidence and deny replay,
reject contradictory success, preserve unrelated capability payloads, and record
an unsuccessful completed report as a failed coordinator task/job.

Public `process-lifetime-SWGvia` reproduced the bug before the outcome fix:
native exit 17 and `data.success:false` became grant success and job completed.
That failed run is not acceptance. Subsequent `process-lifetime-c1zlH3` passed
seven typed-CLI/public-path cases after the outcome fix but before the final
false-success confirmation guard. Exact source receipt reuse starts no work;
the known failed command keeps completed-but-unsuccessful evidence, a failed
job, one attempt and unchanged native identities. Other deadline/permission/
restart cases remain unknown. [Contract and remaining scope](PROCESS-LIFETIME.md#typed-process-authoring-and-unsuccessful-exits-subsequent-increment).

An initial typed-source test incorrectly assumed spaces were invalid in stable
node IDs; the test now uses an actually invalid control character without
changing the existing identity validator. An initial test-only equality compile
error was corrected by comparing serialized evidence, not changing production
types. Neither failed run is counted as passing. Earlier unexplained exits remain
unproven. No installed runtime/release/feed/UI/model changes or signing/security
acceptance follow from these source checks; all full 0.4.0 requirements stay open.

Final-binary `process-lifetime-oSCVSl` passes all seven cases after the stricter
confirmation guard, now using `submit-process` for file-free authoring and exact
source reuse before owner approval. Exit 17 retains grant completion with
`success:false`, `process_exit_unsuccessful`, output/exit evidence and job status
failed. Other non-normal cases remain unknown; no repeated native identities or
second attempt is created. This is local two-logical-node acceptance, not physical
two-PC or interpreter/general pipeline quality.

Fresh scheduling/refusal recovery (`automation-schedules-FuBdWK`), manual
occurrence/native execution/archive (`automation-occurrences-L4ITdv`), definitions/
policies (`automation-definitions-h4ByyH`) and input/answers (`request-input-CcIlfV`)
regressions also pass with the final source binaries.
Reviewed execution/recovery (`request-review-SQF2BM`) also passes; that fixture
uses fake inference, not real interpreter quality or physical two-PC acceptance.
