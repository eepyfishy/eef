# Failed-upgrade rollback: unresolved release gate

Successful installation, upgrade and uninstall do not prove failed-upgrade
recovery. The current standard-user test exposes an unresolved registration
rollback failure. Do not distribute the development MSI as an accepted installer
or relax state assertions to call this safe.

## Separately guarded probe

The non-installing [fixture builder/checker](../tools/msi-upgrade-rollback-fixture.ps1)
copies only a hash-bound canonical synthetic package in its generated fixture.
The source remains immutable. The probe has its own package code and one private
`eef_synthetic_upgrade_rollback` marker. It strengthens the development guard to
require both explicit synthetic rollback flags, adds `InstallExecute` at 6500,
and adds a message-only Type 19 failure at 6550, before finalization at 6600.

Microsoft documents that
[InstallExecute](https://learn.microsoft.com/en-us/windows/win32/msi/installexecute-action)
executes the pending script without ending the transaction, while
[Type 19](https://learn.microsoft.com/en-us/windows/win32/msi/custom-action-type-19)
returns failure without custom executable code. The probe therefore tests failure
after real writes, not an assertion failure before a transaction starts.

The dedicated readback compares every ordinary table against the inspected
canonical package, allowing only those exact differences. It verifies unchanged
embedded cabinet bytes, source digest and other summary properties. The ordinary
development inspector still refuses the probe. It is not an alternative accepted
upgrade recipe, production fault flag, script hook or privilege bypass.

Non-installing authoring checks cover both roles and reject six altered probes
per role (execution position, failure condition/type, file size, package code and
guard), plus source/package digest mismatches. Ordinary 24-case recipe refusal
and six version boundaries remain separate. None of these checks installs MSI.

## Opt-in native acceptance test

Developer test only; it intentionally fails and may leave generated test products
registered if rollback is incomplete:

```powershell
$env:EEF_TEST_ALLOW_MSI_INSTALL = '1'
node tools/test-msi-upgrade.mjs --rollback
```

The normal command without `--rollback` retains its ten-operation lifecycle.
Rollback mode adds one failed upgrade per role before that role's normal upgrade.
Before continuing, it requires the older product fully registered in the exact
current-user unmanaged context, the candidate absent, all older payload hashes
restored and both owner markers unchanged. Native logs must show completed script
execution before the fixed failure, then rollback-script operations after it.
There is no automatic retry, repair, cleanup or reinterpretation of failure.

The helper now writes an immutable `*-native-exit.json` immediately after the
native process exits, before attempting state observation. An observation error
must not discard an already observed exit. Intent, PID/start time, native exit,
state observation and test acceptance remain distinct evidence.

## Retained first failure (2026-10-26)

`msi-upgrade-hVHS7W` installs both older synthetic roles, then intentionally fails
the eef upgrade after `InstallExecute`. The native process terminates with 1603;
file-copy and registry-write operations precede the Type 19 failure. Native
rollback operations restore the older payload hashes and preserve both owner
markers, but product registration is inconsistent:

- Older eef reports advertised state 1 rather than fully installed state 5.
- Failed candidate eef still reports installed state 5 rather than absent.
- The untouched older eefn remains installed; its candidate remains absent.

The log records access-denied errors (5) restoring/removing Installer registration
values during rollback. A separate new-process API read confirms the mismatch;
this is not accepted as an obsolete status snapshot. The exact seven evaluated
source files and original fixture/logs are retained, together with the raw
read-only diagnostic. The observed host is Windows build 26200 with msi.dll
5.0.26100.9711. This identifies the tested environment, not the root cause or a
claim that other hosts are unaffected.

The author already follows Microsoft's
[non-elevating per-user guidance](https://learn.microsoft.com/en-us/windows/win32/msi/authoring-packages-without-the-uac-dialog-box):
no `ALLUSERS` default and summary bit 3 set. No speculative dual-purpose-context
switch, ACL takeover, rollback disabling, registry/GPO workaround, administrator
request or security exclusion is applied. The cause and a supported recovery
design still need investigation and independent acceptance.

A subsequent read-only ACL check finds inherited current-user full control on
the Installer products parent key. It does not support blaming missing parent
access; the removed product child keys' original security is not established.
The retained native log also records both user and machine `DisableRollback`
policies as zero and actually executes rollback operations. Microsoft's
[rollback documentation](https://learn.microsoft.com/en-us/windows/win32/msi/rollback-installation)
distinguishes disabled rollback from attempted rollback; these observations do
not establish a supported root cause or recovery fix. No policy, registry,
ownership or security-setting change is made based on that investigation.

After confirming the failed native PID absent and retaining exact evidence,
three uniquely receipted normal uninstall operations remove only this fixture's
known candidate/advertised-old/node registrations. All return zero. Independent
fresh-process API reads confirm all four product identities absent and marker
hashes confirm the generated owner data survives. However, five old synthetic
payload/inventory files and five scoped component-marker values remain for eef;
these small generated artifacts are retained as forensic evidence. Cleanup is
therefore explicitly registration-only/partial, not a complete payload cleanup.
No manual registry or filesystem removal is used to conceal that additional
failure. This is
explicit test cleanup, **not a successful automatic rollback** or replay of the
failed upgrade. User applications, settings, releases and remote nodes are not
changed. The cleanup receipt is retained with the original failed fixture.

## Late transactional removal experiment (2026-10-27)

Microsoft also documents removal between `InstallExecute` and `InstallFinalize`,
with both the new install and old-product removal inside the rollback boundary.
See [the supported sequence locations](https://learn.microsoft.com/en-us/windows/win32/msi/removeexistingproducts-action).
A fresh private experiment tests that alternative; it is not a public packaging
policy or a claim that the earlier placement caused the failure.

Seven source copies use an explicit experimental recipe identity. The builder
and independent inspector require `InstallExecute` at 6500, `RemoveExistingProducts`
at 6520 and finalization at 6600. The same message-only Type 19 fault stays at
6550, after removal and before commit. The probe retains all original registration,
payload, owner-data, privilege, downgrade and lifecycle assertions, and adds a
check that the deliberate failure follows completed late removal. No executable
payload, custom script action, elevation, security-policy change or new tooling
is introduced. Public source at `a356aa7` remains unchanged during the experiment.

`msi-late-execute-v1/.validation/msi-upgrade-16KW4R` uses a new namespace and
text-only packages. Both older roles install successfully as current-user
unmanaged products. The first eef rollback probe then returns native 1603.
Its log confirms actual file/registry writes, successful `InstallExecute` and
old-product removal, the intentional failure, and subsequent rollback scripts.
The helper preserves the native-exit receipt before failing its state observation.
No later upgrade or eefn rollback attempt is run after that failure.

A separate fresh-process read confirms the same mismatch as the earlier recipe:
older eef state 1, failed candidate eef state 5, untouched older eefn state 5 and
its candidate absent. All old payload hashes and both owner markers match.
The native log records access-denied registration operations under Windows
Installer's `UserData` product paths. This does not establish an underlying
Windows, impersonation, ACL or policy cause. Moving removal alone is therefore
not a verified fix on the tested host; no broader platform conclusion follows.

After confirming the failed process absent, three separately receipted ordinary
uninstalls return zero. Fresh API reads confirm all four experiment product
identities absent and both owner markers unchanged. Five small eef synthetic
payload/inventory files and five scoped component-marker values remain. They
are retained with the logs, not manually deleted to make cleanup look complete.
This is registration-only partial cleanup, not successful rollback. The earlier
failed fixture and user applications are untouched.

`msi-late-execute-v1/independent-audit.json` verifies the seven evaluated source
hashes, unchanged original public sources and MSI bytes, the write/removal/fault/
rollback ordering, original registration failure, native cleanup receipts,
post-cleanup API reads and retained artifacts. Its audit passes while
`native_rollback_accepted` and `real_application_rollback_accepted` remain false.
No public MSI recipe or release is changed by this investigation.

## Post-finalize removal experiment (2026-10-28)

Microsoft also permits old-product removal after `InstallFinalize`. This has a
different failure boundary: a subsequent removal failure rolls back the old
uninstall, not the already-committed new installation. See the
[documented sequence restrictions](https://learn.microsoft.com/en-us/windows/win32/msi/removeexistingproducts-action).
It is not equivalent to an atomic two-product transaction or an accepted policy
change. A distinct private experiment tests pre-commit recovery first.

Seven source copies retain the original context, identity, payload, owner-data,
downgrade, repair and registration assertions. Only the experimental recipe tag
and removal position change: commit at 6600, removal at 6700. The same message-only
fault stays at 6550, after `InstallExecute` and product publication. The unchanged
public inspector rejects all four experimental canonical packages. Payloads are
tiny non-executable text, with a fresh namespace and no elevation or new tooling.

`msi-post-finalize-v1/.validation/msi-upgrade-b3VevJ` returns native 1603 for its
first eef rollback probe. The log proves file/registry writes, publication and
rollback, without reaching commit or executing old-product removal. Fresh API
reads find both old eef and its failed candidate installed (state 5); the older
eefn is untouched and its candidate absent. Nine observed inventoried payload
hashes and both owner markers match. Pre-cleanup restoration of the regenerated
inventory file was not independently established. Registration access-denied
entries persist; the underlying cause remains unproven. No later upgrade or
eefn failure probe runs. Post-commit removal-failure acceptance is not reached.

After retaining that failure and confirming its process absent, three normal
uninstalls remove only the experiment's registrations. Fresh reads confirm all
four identities absent, eleven owned payload/inventory files absent and no scoped
component-marker values, while both generated owner markers remain intact.
This is separately verified cleanup, not successful rollback. Older failed
fixtures and their stranded evidence are not cleaned or reclassified.

`msi-post-finalize-v1/independent-audit.json` reconciles the seven evaluated
source hashes, unchanged public sources/MSIs, publication/write/fault ordering,
fresh native states and cleanup receipts. Initial source-guard and expected-error
collector failures are retained separately; neither caused an installer replay
or relaxed a runtime assertion. The audit passes with `native_rollback_accepted`
and `real_application_rollback_accepted` false. Public packaging and security
settings remain unchanged. Changing removal sequencing alone is not a fix.

## Acceptance still required

At this diagnostic checkpoint, `msi-authoring-9tWuI0` passes six version boundaries,
24 ordinary rejected recipes and eight rollback-probe refusal cases per role,
without installation. A fresh normal-mode fixture `msi-upgrade-EsRuzq` passes its
ten native operations, all ten pre-observation native-exit receipts, 15 payload
observation stages and owner-preserving uninstall. Independent reconciliation
checks 22 ordinary component-value absences and unchanged evaluated source hashes.
The failed fixture, its seven archived source files, registration-only cleanup
and five stranded files/component values remain separate failed-path evidence.
The read-only checkpoint audit is
`.validation/msi-rollback-checkpoint-acceptance.json`; its
`nativeRollbackAccepted` remains false. No new application build or model download
is part of this test/documentation increment.

The new probe is evidence for an open gate, not an implementation that closes it.
Real-application failed-upgrade rollback, installed EXE/updater selection migration,
locked files, normal installer UI/startup, approved model onboarding, second-PC
and signing/Defender acceptance remain open. Successful ordinary lifecycle/CI and
read-only probe checks must not be reported as failed-upgrade acceptance.

## Deferred-script failure experiment (2026-10-29)

A fresh, standard-user, single-product diagnostic tests a different failure
origin: an error inside the execution script after real file writes,
`RegisterProduct` and `PublishProduct`, rather than an immediate Type 19 error
after `InstallExecute`. It starts with no installed, advertised or related
product, uses a new namespace and preserves a generated owner marker.

Only tiny synthetic text is packaged. The private diagnostic adds exactly one
deferred Type 17 + 1024 action, bound to the installed `nested/payload.txt` file.
The source is hash-verified plain text without an MZ header, not a DLL. There is
no executable payload, system-context flag, rollback custom action or fallback
Type 19 fault. The normal package tables, cabinet and summary are independently
compared with their canonical source before any native operation. This is a
failure-injection fixture, not a proposed production custom action. Microsoft's
[Type 17](https://learn.microsoft.com/en-us/windows/win32/msi/custom-action-type-17)
and [deferred-action](https://learn.microsoft.com/en-us/windows/win32/msi/deferred-execution-custom-actions)
contracts distinguish source-file binding from the time of script execution.

The native log confirms actual writes and publication before the queued action's
1723 module-load failure. `InstallExecute` fails and a rollback script runs;
`InstallFinalize` does not complete. Native exit is 1603. Fresh native API reads
still report the candidate installed in current-user unmanaged context. All five
payload/inventory files and scoped component-marker values are already absent,
and owner data is unchanged. The decoded log contains access-denied observations
in both user and machine registration areas. Rollback policy is enabled.

This rules out an immediate Type 19 action as a necessary condition for the
observed stranded registration. It does not prove the engine, package authoring,
ACL ownership, caller token or another environmental factor is the root cause.
The caller's later read-only token inspection finds no restricting-SID list or
AppContainer and does find a filtered, non-elevated token; it does not inspect
the installer worker's token. A [related open upstream report](https://github.com/wixtoolset/issues/issues/8574)
describes similar per-user registration rollback symptoms. That corroborates
symptoms, not a fix or a causal conclusion for these packages.

One separately guarded, ordinary uninstall returns zero for only this synthetic
product. Fresh reads confirm its product/related registrations absent, all five
payload/inventory files absent, no scoped component markers and the owner marker
intact. Eight pinned authoring/execution inputs and both package hashes match.
The read-only audit is
`.validation/msi-deferred-rollback-v2/independent-audit-v3.json`; registration and
real-application rollback acceptance remain false.

An earlier preparation path-literal error occurred before any installation and
is retained separately. Collector errors involving an unsupported output option
and UTF-16LE log decoding are also retained; corrected read-only collectors do
not replay native operations or weaken assertions. Older failed fixtures and
their evidence remain untouched. No ACL, UAC, MSI policy, installation context,
public recipe, real application or tooling change is used to manufacture a pass.
