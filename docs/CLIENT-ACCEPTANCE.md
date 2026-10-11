# Reviewed client acceptance (unreleased after alpha.6)

This checkpoint adapts remaining protocol/browser/model fixtures to the mandatory
[reviewed boundary](EXECUTION-BOUNDARY.md). Both production roles remain at the
unreleased development state after alpha.6. No published installer, update feed,
installed runtime or physical node is changed by this checkpoint.

The 2026-10-11 verification passes 390 workspace/all-target Rust tests both
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
  invoke a keyword fallback. Valid input opens review, not an automatic workload.
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

These checks do not prove interpreter quality/default model suitability, physical
two-PC acceptance, continuous media pipelines, peer transport, cluster safety,
strong termination, MSI onboarding or Defender/Authenticode acceptance. All
remain on the full [active roadmap](ROADMAP-CORE.md); no stable release claim.
