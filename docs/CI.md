# GitHub core verification

## Node-output fixture ownership and failure diagnostics

`2a66c47` [run 40222139226](https://github.com/eepyfishy/eef/actions/runs/40222139226)
is not accepted overall: all four Rust modes and the expanded MSI read-only probe
step pass, but the native sequence stops at its nineteenth suite, node output,
on initial connection/runtime observation. The old hosted trace does not establish
why that observation failed.

A [controlled local reproduction](TRANSPORT-OUTPUT.md#fixture-registration-ownership-follow-up-2026-10-26)
proves that a healthy registered node is misclassified after a probe-only first
socket closes. The fixture now owns the first authenticated registration, not
the first TCP accept, retaining every original bound/no-replay/settings assertion.
Fifteen ownership/privacy/process-observation cases, both distinct native paths
and a missing-executable diagnostic negative case pass locally with unchanged
application artifacts. No production networking or model policy changes.

Every matrix job runs the pure fixture-state test. Windows headless keeps all
thirty main suites and separately runs the injected pre-registration-close case
once after them, for 31 native executions. Failure diagnostics are whitelisted
metadata in the job log; raw process/error evidence stays private/ignored and is
not uploaded. Fresh hosted acceptance must verify both the exact main order and
the additional completion marker. Old failed runs are never converted to passes.

## In-development rollback probe: read-only checks are not recovery approval

The Windows authoring test adds exact synthetic rollback-probe construction and
eight refusal cases per role without installation or payload execution. Its
dedicated inspector checks unchanged source/payload bytes and exact allowed
differences; the ordinary inspector refuses those deliberate-failure variants.
The separate opt-in standard-user post-write test exposes a registration recovery
failure, documented in [rollback validation](MSI-ROLLBACK-VALIDATION.md); that is
an open gate, not a passing hosted test. Fresh CI of these new sources is separate.

Published `eb3f4ef` passes all four matrix jobs and the exact thirty native suites
in [run 40219615827](https://github.com/eepyfishy/eef/actions/runs/40219615827), including
six version boundaries and 24 rejected ordinary authoring recipes per role.
Its acceptance record is `.validation/msi-major-upgrade-ci-acceptance.json`.
It does not verify the later probe, failed-upgrade recovery or stable readiness.

## In-development MSI major-upgrade recipe

The native authoring step now includes the [major-upgrade-v1](MSI-UPGRADE-POLICY.md)
contract and 24 refused recipes per role, retaining all thirteen prior cases.
Six native version boundaries, thirteen pure version-policy cases/48,640 ordinals
and thirty-five planner cases remain. Hosted authoring does not install an MSI.
The separate opt-in standard-user synthetic lifecycle and independent audit pass
ten native operations locally, without executing an app payload; that is not an
extra hosted test or production migration/rollback approval. Fresh exact-revision
hosted verification of the new packaging source remains pending.

The application baseline `7df79a1` separately passes all four matrix jobs and the
exact thirty Windows native suites in
[run 40206970109](https://github.com/eepyfishy/eef/actions/runs/40206970109), including
the seven repair-readiness cases and original MSI version-policy checks. Its
read-only audit is `.validation/atomic-reader-ci-acceptance.json`. The retained
`fcd559f` failure below is historical, not the result of that accepted follow-up.

## Retained version-policy run failure and atomic-reader follow-up

`fcd559f` [run 40198635175](https://github.com/eepyfishy/eef/actions/runs/40198635175)
is not accepted overall. Three jobs, all four Rust regressions and the Windows
MSI authoring/policy checks pass, but the Windows native sequence stops at
`test-repair-receipts.mjs`: a valid admitted snapshot is mistaken for a downloading
snapshot after a later network event. The readiness correction keeps exact state
assertions/deadlines. Repeated local verification also exposes a distinct Windows
atomic-publication/read collision; its retained failure and shared backend fix
are tracked in [atomic settings](ATOMIC-SETTINGS.md#windows-live-reader-follow-up-in-development).
Neither failure is converted into success by rerunning the old hosted revision.
Both full local Rust modes of the follow-up pass 1,138 tests plus one ignored.
All thirty hosted-selection native suites, two cached-runtime suites, five
additional distinct receipt fixtures and the unchanged first-run browser
regression pass against frozen source and eleven native artifact hashes.
The separate shared-source stress test completes 5,000 publications without
incomplete reads; a cached real-CPU workflow and independent read-only audit also
pass. A retained standalone harness loader failure is corrected only through its
process-local compiler-runtime search path, without replaying completed suites.
New exact-revision hosted acceptance remains separate and pending.

## Current in-development MSI version-policy regression

The [packed-patch-v1 contract](MSI-VERSION-POLICY.md) adds thirteen pure policy
cases and 48,640 checked ordinals before the now thirty-five-case payload planner
in every matrix job. Windows native authoring also verifies six independent
inspector boundaries and thirteen refused recipes per role. The local
source-hash-locked run and independent read-only audit pass; retained real
experimental packages are diagnosed only through explicit legacy opt-in.
Hosted verification of this new increment remains separate. No installation,
application rebuild, model download or automatic-update change is part of it.

## Lowercase project naming

Maintained documentation, comments, UI and installer messages use `eef` and
`eefn`, including at the beginning of sentences. The shared CI check runs
`node tools/test-project-names.mjs` and `node tools/check-project-names.mjs`.
Mixed-case branding is rejected. Exact legacy machine-interface exceptions
preserve verification prefixes, installed-package recognition and discovery
paths; environment-variable identifiers remain compatible. User-owned future
drafts and historical generated artifacts are not rewritten.

## Accepted native authoring/naming checkpoint (2026-10-25)

Its documentation-only follow-up `75bf6a7` also passes all four jobs and the exact
thirty-suite native audit in
[run 40196510127](https://github.com/eepyfishy/eef/actions/runs/40196510127).
That commit changes no application/tool source; it records the controlled local
lifecycle. Neither run accepts later version-policy edits.

Published `f9951af` passes all four matrix jobs in
[run 40194073272](https://github.com/eepyfishy/eef/actions/runs/40194073272).
The exact-revision audit verifies the thirty native Windows suites in dispatch
order, thirty-four planner cases, successful Windows development MSI authoring
and the shared naming checks. No installer is executed in that hosted authoring
step. Separate local current-application browser and
[controlled MSI lifecycle](MSI-PACKAGING.md#current-application-lifecycle-checkpoint-2026-10-25-development-only)
evidence covers standard-user install/repair/uninstall of both roles; those local
observations are not additional hosted tests or stable-release acceptance.

## Accepted first-run and read-only MSI planning checkpoint (2026-10-25)

The application increment passes both full local Rust modes (1,132 tests plus one
existing ignored profiler each), all thirty-two native Windows suites, a separate
cached real-CPU owner workflow and the unchanged default-feature browser regression.
Each app epoch has independent source/artifact/process evidence. The original
package-setup timeout is retained; the full corrected run uses the unchanged
deadline after a streamed-hashing correction, not a timeout relaxation.

Windows headless CI now selects thirty native suites by adding managed first-run
initialization. Package-doctor and unavailable-model controls still require the
existing verified package/runtime and remain local-only. The separate read-only
MSI payload planner has thirty-four local synthetic cases and an explicit test
step in every matrix job; it launches no payload, installs no MSI and downloads
no model. Published `4ea7964b47c6db5cab990f391a9e368b5c9c87d4` passes all four
hosted jobs in [run 40176167808](https://github.com/eepyfishy/eef/actions/runs/40176167808).
The independent exact-revision audit confirms the thirty Windows native suites
in dispatch order, successful regression and planner steps in all four jobs,
and the explicit thirty-four-case planner output. Its record is
`.validation/initialization-ci-acceptance.json`. Later development MSI authoring
and inspection changes are separate, not accepted by this older run.
No version, release asset, stable feed or installed node changed.

## Accepted optional Granite catalog follow-up (2026-10-25)

The catalog-only change passes both full local Rust modes (1,107 tests plus one
existing ignored profiler each), eight focused model/native suites and the real
CPU lifecycle workflow with unchanged evaluated hashes. Both frozen quality
suites finish but reject default candidacy: 11/20 and 6/20 label matches, all
fifteen clear inputs questioned, twenty-five copied example goals. No default,
prompt/schema/validator or permission change follows. This does not claim all
thirty-one native suites for the data change or hosted acceptance from the
preceding shell revision. Its own published revision
`c1668d8219e0c3dffa39fed1084e8e7ca963c448` passes all four hosted jobs and the
exact twenty-nine-suite Windows native dispatch in
[run 40146881809](https://github.com/eepyfishy/eef/actions/runs/40146881809).
The independent read-only record is `.validation/granite-catalog-ci-acceptance.json`.
Later first-run edits require their own acceptance; no release or model-quality
approval follows.

## Accepted Windows shell checkpoint (2026-10-25)

The new Windows PowerShell native suite exercises ordinary command I/O/status
and selected-package command handoff. Configured Windows native suites increase
to twenty-nine; the previously accepted hosted revision below ran twenty-eight.
Published `6aa2adec510462f555cd34a1ec1f3ec71b96c64b` passes all four jobs in
[run 40145011857](https://github.com/eepyfishy/eef/actions/runs/40145011857).
Its completed Windows headless log confirms all twenty-nine configured native
suites in exact order; `.validation/windows-console-ci-acceptance.json` records
the independent pinned-revision audit. This accepts that source, not subsequent
catalog/experimental edits or a release.
Both full local modes pass 1,107 tests plus one existing ignored profiler each,
all thirty-one native suites pass against unchanged evaluated hashes, and the
independent cached real-CPU regression passes. Its twenty-four shell cases cover
legacy one-shot options and nonempty stdin propagation as well as selected child
exit/output and two API-live quiet services. The earlier pre-classification run
was deliberately interrupted for a real legacy-flag correction, not accepted.
Read-only records are `.validation/windows-console-acceptance.json` and
`.validation/windows-console-real-cpu-acceptance.json`.

## Accepted backend-selected start/readiness checkpoint (2026-10-25)

Both full local feature modes pass 1,104 tests plus one existing ignored profiler
each, all thirty native suites pass sequentially, and the independent cached
real-CPU regression passes against unchanged evaluated hashes. The read-only
records are `.validation/model-check-readiness-acceptance.json` and
`.validation/model-check-readiness-real-cpu-acceptance.json`.
The native model-check fixture now covers CLI/backend ownership, typed preflight
refusals without a source, and retained selected starts offline and after model/
runtime changes. Published `1babe4f434a12d706d18442dbcb78b756c3a3426` passes all four
matrix jobs in [run 40139874670](https://github.com/eepyfishy/eef/actions/runs/40139874670).
The completed Windows headless log confirms all twenty-eight configured suites
in exact order; `.validation/1babe4f-ci-acceptance.json` records the audit. This
accepts that revision, not the later shell/source changes.

## Accepted explicit model-check checkpoint (2026-10-25)

The current workflow adds `test-model-verification.mjs`, bringing
its selected native suite count to twenty-eight. That fixture covers explicit
owner inference consent, immutable node-authored test input, lost-reply receipt
inspection without replay, empty output, consent revocation and both restarts.
Focused native and real CPU validation pass. Both full local feature modes pass
1,099 tests plus one existing ignored profiler each against unchanged evaluated
source hashes. Fresh headless app/helper build and all thirty native suites pass
in one sequential run with all 302 evaluated source/resource/workflow and nine
native artifact hashes unchanged. No matching fixture processes remain observed.
The local record is `.validation/model-verification-acceptance.json`. Published
source `a6752ad9648ea6412c0daf6e57fa28cfbe970f42` passes all four matrix jobs in
[run 40124073155](https://github.com/eepyfishy/eef/actions/runs/40124073155).
The completed Windows headless log confirms all twenty-eight configured native
suites in exact order; `.validation/a6752ad-ci-acceptance.json` records the
read-only audit. This supersedes the pending hosted status of that checkpoint,
not acceptance of the subsequent backend-selected start/readiness edits.
Real CPU, physical and release acceptance remain separate.

## Accepted maintenance/retirement checkpoint (2026-10-24)

Published source `fd21f14721c11f62a730b8f5a06335b026a5ca3c` passes all four matrix
jobs in [run 40076717528](https://github.com/eepyfishy/eef/actions/runs/40076717528).
The completed Windows headless job log confirms all twenty-seven selected native
suites in exact configured order. The read-only record is
`.validation/fd21f14-ci-acceptance.json`. This supersedes the pending hosted status
of that source, not the status of future worktree edits. Separate real CPU
lifecycle/inference passes too. Neither result approves interpreter quality,
project defaults, automatic onboarding, physical two-PC, MSI/signing/Defender or
stable release readiness.

## Workflow and earlier checkpoints

The `Core verification` workflow runs on pushes to `main` and `validation/**`,
pull requests targeting `main`, and manual dispatch. Each run checks out its
triggering revision, not a developer's uncommitted worktree.

The matrix tests Ubuntu 24.04 and Windows 2025 with default dashboard features
and with `--no-default-features`. All four jobs check formatting, the
backend-to-browser dependency boundary, and the locked workspace/all-target
Rust regression suite. Rust is pinned to 1.98.1 and Node to 24.20.0.

The current Windows headless job additionally builds command services and example
helpers, then selects 29 sequential native synthetic regression scripts. Python
3.11.9 is selected explicitly for the Python fixtures. These tests use isolated
fixture configuration, loopback services, and synthetic artifacts, not installed
user nodes or downloaded language models. The workflow file is the authoritative
list of included suites; future feature tests must be added explicitly.

The execution/reply ownership increment adds blocked-output/reconnect,
reviewed-request and input-workflow suites. The accepted `730ad26` run below
covers all twenty-six. Earlier accepted runs cover twenty-three native suites,
not these additional three.

The package-policy/retirement increment selects one additional native suite and
extends model activation/bootstrap regressions. Its local acceptance passes
1,089 tests plus one existing ignored profiler per feature mode and all
twenty-nine native suites, including the two stock-runtime local-only suites.
No published 27-suite acceptance is claimed until that revision's hosted run
completes. The accepted 26-suite baseline below remains revision-specific.

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

## Reply-ownership and setup-stop checkpoint (2026-10-24)

[Run 40069111052](https://github.com/eepyfishy/eef/actions/runs/40069111052) passes
all four matrix jobs and all twenty-six Windows native suites for
`730ad260933af9c60a2a3e39d3b39c7e7323841f`. The successful terminal jobs and exact
native dispatch order are checked in `.validation/730ad26-ci-acceptance.json`.
Separate local acceptance passes 1,073 tests plus one ignored per feature mode
and twenty-eight native suites, including the two stock-runtime tests not claimed
by hosted CI. The isolated real CPU regression also passes at that source.
Neither CI nor that operation test approves a default/interpreter's quality,
complete automatic onboarding, physical two-PC, MSI/signing/Defender or stable
release readiness. No version, installed node, release asset or feed changed.
