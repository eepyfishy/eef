# Read-only node setup check (unreleased)

Subsequent [registration receipts](REGISTRATION-RECEIPTS.md) distinguish EEF's
nonce/runtime/session-bound acceptance from a local registration write. Setup,
status and diagnostics expose that observation without inference or mutation;
older peers/reports remain unknown, never assumed accepted or work-ready.

```text
eefn setup
eefn --json setup
```

The first command explains setup in plain language; the second returns the same
versioned report for a client. Both inspect the already-running node selected by
its existing config/actual local API marker. No port number, JSON input file,
browser, coordinator connection or model is required to run the inspection.
If the node is stopped, the command asks the owner to start it; it never launches
a temporary second runtime or connection.

## Shared backend and observations

Clients call the node-local owner service:

```text
POST /api/commands/setup/check
{
  "schema_version": 1,
  "expected_node_id": "EXISTING_NODE_ID",
  "expected_runtime_id": "CURRENT_NODE_RUNTIME_UUID",
  "request_id": "NEW_CORRELATION_UUID"
}
```

The body is capped at 4 KiB. The CLI discovers the actual local API and checks
diagnostics before sending this request. The backend validates the exact
node/runtime and returns a bounded
`node_setup_check` report. Browser cross-origin/DNS-rebinding guards are the
existing local-owner boundary. The dashboard may consume this service; it does
not own readiness decisions, restart logic or model selection.

The report separates:

- [Local input channel](INPUT-CHANNEL-READINESS.md): unavailable, ready or busy.
  This observes only the existing bounded queue, not delivery/approval/readiness
  of models or a coordinator. Missing data from older builds stays unknown.

- Connection state and observed transport (`pinned_tls13`, `legacy_shared_key`
  or unknown). Connected legacy transport is not reviewed-work readiness.
- Whether the current node runtime reports configured private execution receipt
  support. Explicit null means unknown, not false or available. This is a local
  configuration fact, not coordinator readiness, approval, a fresh reservation,
  successful dispatch or receipt durability under every failure condition.
- Saved interpreter choice and applied interpreter candidates. The latter uses
  the **same selector as actual interpretation**, not advertised peer models or
  a model-name heuristic. Missing, selected, ambiguous and unavailable runtime
  are distinct. A retired/expired/mismatched preview binding is unavailable.
- Whether a known, unambiguous saved choice matches its applied counterpart.
  Ambiguity/unavailable runtime yields null, not guessed equality.
- Saved authentication/connection-enabled preference, explicit pending-restart
  observation and
  bounded known startup-issue codes. Unknown pending state remains null.

Inspection is point-in-time, not an atomic snapshot of config, connection and
runtime replacement. Data may change after the reply. No model process is reaped,
health endpoint probed, inference generated, capability enabled, file deleted,
download/model load requested, update applied or app restarted. No workload or
execution grant is issued. An applied role means **configured**, not healthy,
installed-artifact verification, acceptable model quality or successful inference.

## Next steps, compatibility and privacy

Ordered next-step codes are derived in the backend and validated by the adapter:
resume a paused connection, check EEF, private pairing, inspect receipt transport,
choose one interpreter, apply saved settings explicitly, wait for runtime startup
or inspect bounded startup issues. An already-saved private pairing awaiting
restart asks for applying settings,
not a second invitation or credential rotation. Without a pending restart,
missing receipt support on that saved private setup asks for transport inspection.
Plain-language rendering uses fixed templates; it does not print arbitrary
node-provided prose or treat suggestions as shell
commands to execute. Suggested actions still require the existing owner commands
and their permission/current-state checks. Restart applies all pending settings,
not just a model role; inspection never performs it automatically.

Successful inspection sets `success: true` even when setup is incomplete. All
mutation/inference/restart/execution/retry flags remain false. Do not use this
field as an "assistant works" health badge. No convenience action auto-grants
permissions or bypasses request review, model validation or pinned authentication.

The report contains node/runtime/correlation IDs and selected model IDs for
owner-side use. It omits PSKs, certificates, addresses, hostnames, paths, raw
startup errors and personal request history; no automatic upload occurs. Model
IDs remain owner-provided labels, not guaranteed non-sensitive data. Treat them
accordingly when exporting diagnostics.

Strict report validation rejects substituted targets/correlation, unknown fields,
extra fields on tagged states, malformed/missing required fields, altered fixed
notes, inconsistent equality/actions and claimed side effects. Lost, corrupt or
unsupported replies fail without fallback or automatic retry. HTTP 404 asks for
a compatible node build; it does not call old generic config/UI endpoints.

## Verification (2026-10-14)

- The final default and dashboard-disabled workspace/all-target suites each
  passed **665 tests**, with one ignored. Five focused setup cases cover missing/
  retired runtime, saved/applied choices, ambiguous/paused state, private pairing
  pending apply, and strict report/target/privacy checks. The initial default
  pass had 664 tests before the saved-authentication guidance fix; the final
  source was rerun, rather than treating the earlier count as coverage of it.
- Fresh command binaries/examples, formatting, diff whitespace, changed
  JavaScript syntax and the backend/browser source-boundary check passed.
- `setup-check-Q6plSM` passed headless plain-text/JSON inspection through the
  actual running-node commands: legacy connection versus private receipt
  support, saved/applied interpreter selection, explicit restart/private pairing,
  pause/resume, wrong/stale targets, cross-origin rejection, 4 KiB body bound,
  dropped/substituted/unsupported replies without retry/fallback, and refusal to
  launch a stopped node. Config bytes remain unchanged during each inspection;
  the synthetic backend receives no inference/load/download request.
- `request-preview-mgQ8d5` passed the existing public preview regression after
  the selector was shared with setup inspection. This uses a synthetic model,
  not a model-quality or real-inference benchmark. Fixture processes exited.

These are isolated localhost tests. They do not prove physical two-PC behavior,
automatic bootstrap/onboarding or release acceptance. Earlier local checkpoint
passes are not substituted for verification of the final source.

## Remaining setup and release requirements

This addresses understanding current setup without knowing architecture/ports.
It does **not** complete private first-run pairing orchestration, automatic
verified/default interpreter bootstrap, model quality, permission/setup wizard,
MSI/Defender/signing, continuous/peer work or physical two-PC acceptance. Current
published installers and installed nodes remain unchanged; stable v0.4.0 is not
claimed. Defaults and automatic update selection still exclude prereleases.
