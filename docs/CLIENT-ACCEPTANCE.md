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
