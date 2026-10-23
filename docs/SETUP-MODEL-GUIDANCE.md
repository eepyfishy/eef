# Shared node setup guidance (unreleased after alpha.6)

The existing node setup check now explains the model-setup backend's retained
state instead of asking for another interpreter while an approved setup exists.
EEF can inspect the same backend over its existing authenticated node channel:

```
eefn setup
eef node setup --node NODE_ID
```

These are observations, not setup approval or a readiness test. They never select,
download, load or infer with a model, change permissions, restart an app, enable
automation or replay work. The configured model and interpreter are still shown
separately from applied runtime choices. Explicit commands remain usable without
an interpreter or an available setup journal.

## Guidance belongs to the backend

| Observation | Next step |
| --- | --- |
| Owner-pinned prompt setup awaits consent | Inspect the pin and explicitly approve one attempt with the existing policy command. The check never approves it. |
| Current setup has a retained active checkpoint | Inspect that exact setup/installer receipt, not choose another model or interrupt this runtime with a suggested restart. A checkpoint does not prove a live worker or current byte progress. |
| Setup outcome failed/unknown/stopped | Inspect retained evidence before a new explicit decision; never assume rollback or safe automatic retry. |
| Automatic restart request retained in this runtime | Inspect the existing request; do not suggest a duplicate restart. It is not completion or physical-stop proof. |
| Saved configuration needs manual application | Preserve the existing explicit restart guidance; the check cannot apply unrelated pending settings. |
| No approved project default, changed catalog or unavailable evidence | Explain the actual blocker, without choosing a catalog entry, repairing evidence or disabling core controls. |

Older-policy/runtime attempts are labelled historical, not current progress.
Existing selections and owner opt-outs remain authoritative. No project default
or model quality is approved by this increment. Model inference/live dependency
health and complete first-run onboarding remain separate requirements.

The new optional `model_setup` observation reuses the existing validated saved
policy and setup-status types. It includes no provider URLs, file paths,
credentials or raw errors. Missing legacy metadata stays unknown; it is not
interpreted as disabled automation. Unsupported/corrupt observations are refused,
not downgraded to an older unchecked report. Older clients may reject the new
optional observation/next-step values and need updating; no fallback is sent.

## Read ownership and remote boundaries

Local HTTP and authenticated remote setup adapters share one owned blocking read
slot. Configuration/journal I/O does not run on the async control thread. A
cancelled caller cannot release that slot while its read is still queued/running;
additional inspections fail busy rather than accumulating workers. The read uses
the existing bounded read-only journal access and never creates a missing sidecar.
Runtime identity is checked again after collecting the observations.

The EEF command/API optionally resolves the node runtime once, then sends one
typed setup inspection with the original node/runtime/correlation identities.
An explicit expected runtime is never replaced. Lost, mismatched, corrupt or
unsupported replies do not trigger another inspection, legacy get, setup action
or raw-error exposure. Read-only inspection does not enable or require the local
remote-write consent switch. Approval/configuration continue to require their
existing separate current consent and policy/runtime fences.

`POST /api/commands/node/setup` retains the coordinator's owner/origin guard and
has a 4-KiB body limit. The node reserves bounded control-request capacity for
setup inspection, not model-setup mutations or workload execution. The dashboard
is not a controller and has not been changed.

## Validation boundary

The previous binary reproduces the misleading interpreter-choice guidance during
a held synthetic Ollama transfer. The failure is retained in
`.validation/setup-model-status-baseline.log`; it is not a successful setup run.

Before the subsequent doctor hashing follow-up, both workspace/all-target feature
modes passed 1,057 tests plus one ignored each against 294 unchanged source hashes.
Their retained logs are `.validation/setup-guidance-headless-before-hash-fix.log`
and `.validation/setup-guidance-default-before-hash-fix.log`. An earlier focused
run passed 84 filtered tests, but predates the final restart-deferral and explicit
remote-runtime regressions. Fresh headless apps/helpers built. The changed native
model-setup (`model-setup-7HbYxP`) and setup-check (`setup-check-9TsXqa`) fixtures
then passed, covering:
local/EEF guidance during a retained download, approval and unknown states,
inspection with remote writes disabled, runtime/origin/body fences, legacy/malformed
metadata, no inference and no configuration changes. The broader sequential run
stopped in package doctor (`package-doctor-PpSupu`): its one-second probe returned
the expected deadline result, but the whole command exceeded the unchanged
eight-second harness bound. The failure remains in
`.validation/setup-guidance-native-failed-doctor.log` and the private fixture.
It is not a passing native run. The subsequent hashing implementation/equivalence
regression required fresh combined validation, recorded below; earlier passes
were not substituted for it. Physical two-PC, model quality, full first-run,
MSI/signing/Defender and stable-release acceptance remain open. The separately
published GitHub workflow (`c00e3d4`) passes all four matrix jobs and twenty-three
Windows native suites in [run 39958661651](https://github.com/eepyfishy/eef/actions/runs/39958661651).
It does not include this subsequent backend checkpoint. No installed node, model,
version, release artifact or
update-feed policy changed.

Both fresh hashing follow-up feature modes now pass 1,058 tests plus one ignored
each, including independent digest equivalence; all 294 source hashes still
match. An earlier default-feature process disappeared without a terminal result
or remaining validation owner; it is not accepted. The later completed default
run is separate evidence. Fresh headless apps/helpers build, then all twenty-five
native suites complete in one sequential run, including the unchanged doctor
observation bound. The terminal process exits zero. The read-only acceptance
inventory confirms every public result is passed, all 294 source/resource and
nine native artifact hashes match, and no matching fixture processes remain
observed. Evidence is `.validation/setup-guidance-native-complete.log`,
`.validation/setup-guidance-acceptance.json`, and the two `*-final.log` Rust logs.
This establishes local checkpoint acceptance, not a stable release.
