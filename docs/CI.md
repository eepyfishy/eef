# GitHub core verification

The `Core verification` workflow runs on pushes to `main` and `validation/**`,
pull requests targeting `main`, and manual dispatch. Each run checks out its
triggering revision, not a developer's uncommitted worktree.

The matrix tests Ubuntu 24.04 and Windows 2025 with default dashboard features
and with `--no-default-features`. All four jobs check formatting, the
backend-to-browser dependency boundary, and the locked workspace/all-target
Rust regression suite. Rust is pinned to 1.98.1 and Node to 24.20.0.

The Windows headless job additionally builds command services and example
helpers, then runs 26 sequential native synthetic regression scripts. Python
3.11.9 is selected explicitly for the Python fixtures. These tests use isolated
fixture configuration, loopback services, and synthetic artifacts, not installed
user nodes or downloaded language models. The workflow file is the authoritative
list of included suites; future feature tests must be added explicitly.

The execution/reply ownership increment adds blocked-output/reconnect,
reviewed-request and input-workflow suites. This source checkpoint needs a fresh
hosted run; the accepted historical runs below
cover twenty-three native suites, not these additional three.

Each job fetches the locked dependency graph before offline builds/tests.
Official checkout, Node setup, and Python setup actions are pinned by full
commit SHA. Dependency caches and artifact uploads are not enabled. Private
fixture journals, credentials, and child-process logs must not be uploaded.
Jobs fail on the first failing step; matrix jobs are independent and are not
automatically retried. Superseded runs on the same ref are cancelled.

The workflow has only `contents: read`, does not persist checkout credentials,
and does not use project authentication secrets. Pull requests run through
`pull_request`, never a privileged `pull_request_target` execution path. It
cannot publish releases, sign installers, update nodes, or modify update feeds.
The project's isolated maintainer login needs `workflow` scope to publish the
workflow file; this is separate from the CI job's read-only token permissions.

## Acceptance boundaries

Green CI verifies only the selected revision and included synthetic checks.
It does not establish useful model quality, physical two-PC acceptance,
successful public package repair, fresh installation/migration/uninstallation,
Defender clearance, code signing, or stable v0.4.0 readiness. Package-doctor and
unavailable-model native suites still require separately verified local runtime
artifacts and are not included in this workflow. The ignored latency profile is
not a CI performance guarantee.

CI setup does not change the application version or prerelease exclusion from
automatic updates. Existing historical notes saying CI stayed local describe
their checkpoints; this workflow supersedes that policy once published.

## First-run finding (2026-10-23)

The workflow was published at `5ecb9be`. Both Ubuntu legs of the first run
(`39936260990`) exposed a platform-timing assumption in the group-stop ownership
regression: after a late stop observation, Tokio may already have reaped the root
and cleared `Child::id()`, although the owned record/tree remains retained and
the model stays `unload_unconfirmed` and unroutable. The test had required the PID
to remain present. The corrected assertion requires one retained record for the
same model, either its original live PID or explicit observed root exit, and
preserves the subsequent cleanup/exit-observation checks. No platform skip,
production stop change, relaxed deadline or retry is introduced. Fresh CI results
are required; the initial failed run is not a pass.

The follow-up run (`39937079093`) passed all four Rust matrix legs. Windows native
fixtures passed scheduling and package setup, then failed creating repair metadata
because `Get-FileHash` could not be resolved by their `powershell.exe` child.
The `pwsh -> Node -> Windows PowerShell` launch chain matches Microsoft's
[documented inherited-module-path incompatibility](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_psmodulepath).
The native step now removes only its inherited process-local `PSModulePath`, so
Windows PowerShell rebuilds its own default module paths. A Node-to-PowerShell
cmdlet preflight checks that exact launch path before any suites run. No modules
are installed, global environment/security settings changed, tests omitted or
failure/timeout conditions weakened. The corrected native result remains pending.

Run `39938587145` again passed every Rust matrix leg and the native cmdlet
preflight, but the repair-descriptor fixture still could not resolve the cmdlet.
Resetting only at step entry was therefore insufficient; it is not counted as a
fix. The complete Windows native step now uses Windows PowerShell 5.1, matching
the fixture helpers and the local native-validation host. It verifies that host
edition/version before running, retains the module reset and exact Node-child
preflight, and avoids returning to a PowerShell 7 parent that can initialize its
own module search paths later. The remaining matrix steps continue using `pwsh`.
Fresh native results are still required.

## Accepted published baseline (2026-10-23)

[Run 39958661651](https://github.com/eepyfishy/eef/actions/runs/39958661651) passes
all four matrix jobs at `c00e3d4f7b5b5d779fe1ddf89f190b110589be01`. The Windows
headless job executes all twenty-three native suites in their configured order
and completes successfully, including the repair-descriptor fixture that failed
in the previous runs. The entries above retain interim failures and incomplete
fixes; they are not the current acceptance status. The read-only record is
`.validation/ci-39958661651-acceptance.json`.

This verifies that committed baseline, not the subsequent setup-guidance/
doctor-hashing checkpoint, physical acceptance or stable-release readiness.

## Subsequent backend checkpoint

[Run 39961728033](https://github.com/eepyfishy/eef/actions/runs/39961728033) passes
all four matrix jobs and twenty-three Windows native suites for the published
setup-guidance/doctor-hashing source at
`f4e4c665a44a679b461ec994de0d4c88c7523149`. The dispatch order and successful
terminal jobs are checked in `.validation/f4e4c66-ci-acceptance.json`. Separate
local verification passes 1,058 tests plus one ignored per feature mode and all
twenty-five native suites; the additional real CPU lifecycle/inference test is
documented in [model lifecycle](MODEL-LIFECYCLE.md). None establishes approved
interpreter quality, complete automatic onboarding, physical two-PC, installer/
signing/Defender or stable-release acceptance.
