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
