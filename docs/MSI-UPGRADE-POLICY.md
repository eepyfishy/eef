# MSI installed-version policy: major-upgrade-v1

This is the development package's native transaction recipe, not production
migration or release approval. It builds on [packed-patch-v1](MSI-VERSION-POLICY.md).
The project namespace and role identify a product family; canonical source
versions identify separate products in that family. Forks use their own namespace.
eef and eefn keep distinct upgrade codes, component identities and directories.
No repository account, computer name or contributor identity is embedded.

## Native behavior

The [author](../tools/build-development-msi.ps1) writes the lowercase private
`eef_msi_upgrade_policy` marker as `major-upgrade-v1`. The
[independent inspector](../tools/inspect-development-msi.ps1) checks that contract
without invoking Node, starting an installer session or changing installed state.

Two exact [Upgrade table](https://learn.microsoft.com/en-us/windows/win32/msi/upgrade-table)
ranges use the package's own role/namespace upgrade code and encoded version:

| Range | Attributes | Action property | Purpose |
| --- | --- | --- | --- |
| `0.0.0 <= installed < package` | MinInclusive (256) | `EEF_OLDER_PRODUCTS` | Eligible older products |
| `installed > package` | OnlyDetect (2) | `EEF_NEWER_PRODUCTS` | Downgrade refusal only |

Language is unrestricted; removal and ignore-failure overrides are not authored.
Both action properties are listed in `SecureCustomProperties`, not initialized
to arbitrary package values. Equal versions are not classified as older/newer;
native maintenance applies to the same product identity. Same-version repackaging
and minor/small upgrades are not thereby accepted.

The development consent guard still runs first, at 100. Both UI and execute
sequences then run `FindRelatedProducts` at 200 and the exact
`eef_block_downgrade` error at 201, conditioned on
`NOT Installed AND EEF_NEWER_PRODUCTS`. This follows Microsoft's
[major-upgrade downgrade detection](https://learn.microsoft.com/en-us/windows/win32/msi/preventing-an-old-package-from-installing-over-a-newer-version)
pattern. The sole custom action is MSI Type 19 with a fixed error message:
no executable, DLL, script, shell, network operation or administrator request.
Maintenance/uninstall is not rejected merely because another version exists.

`RemoveExistingProducts` is at 1520, after `InstallInitialize` (1500) and before
`ProcessComponents` (1600). This is one of the
[documented in-transaction placements](https://learn.microsoft.com/en-us/windows/win32/msi/removeexistingproducts-action).
Older package-owned files are removed before installing replacements, avoiding
unversioned-file date heuristics. No broad removal glob or mutable configuration
component is added. This placement alone is not evidence of successful rollback;
real failed-upgrade recovery needs its own native/application tests.

## Structural refusal and legacy diagnostics

Inspection verifies exactly two ranges, their flags/bounds/codes, the secure
property list, one exact message-only action and both complete action sequences,
including conditions and positions. Unknown policies are always refused. Missing
upgrade policy requires explicit read-only `-AllowLegacyExperimentalUpgrade`;
that diagnostic reports `upgrade_recipe_verified: false`. Packages also missing
the older version marker additionally require `-AllowLegacyExperimentalVersion`.
Neither switch modifies, installs, converts or approves historical artifacts.

`upgrade_recipe_verified` means structural readback only. Authoring/inspection
reports retain `installed_upgrade_verified: false`, migration/rollback/ordering
approval and release flags false. A valid table is not an observed installation.

## Opt-in lifecycle regression

The separate [failed-upgrade rollback gate](MSI-ROLLBACK-VALIDATION.md) is currently
unresolved: a real synthetic post-write failure restores files but not exact
product registration on the tested standard-user host. The successful ordinary
lifecycle below must not be used to claim that failure path is safe.

```powershell
node tools/test-msi-authoring.mjs
$env:EEF_TEST_ALLOW_MSI_INSTALL = '1'
node tools/test-msi-upgrade.mjs
```

Authoring remains non-installing and tests both roles, six independent version
boundaries and 24 rejected recipes per role (13 prior cases plus 11 upgrade
mutations). It does not execute payloads.

The separate lifecycle test requires Windows and a standard-user token, explicit
consent, a newly generated namespace and immutable operation receipts. Its four
packages contain only synthetic non-executable bytes. It first installs both
roles, upgrades each from alpha to beta, attempts the older package and requires
the intended Type 19 refusal before transaction initialization, explicitly
reinstalls the same newer package, then uninstalls both. Between phases it checks
every payload hash, exact current-user unmanaged registration, role isolation and
an unowned data marker. Final checks require all products/package-owned files
absent and owner markers unchanged. No model, real application or user install
is used. The guarded [native helper](../tools/msi-upgrade-fixture.ps1) refuses
unknown related products and never automatically retries, elevates or reboots.

On failure, retain the exact fixture, process/operation receipts and current
installed state. A timeout is not permission to replay installation or launch
cleanup while the first transaction may still be active.

The final local fixture `msi-upgrade-yS5fse` passes all ten native operations:
eight successes and two intended downgrade refusals. Independent read-only
reconciliation verifies all four products absent, 22 component-value absence
checks, 15 payload/owner observation stages and both retained marker hashes.
Its six evaluated source hashes remain unchanged; authoring separately passes
six boundaries and 24 negative recipes per role in `msi-authoring-NnpWpu`.
The prior lifecycle fixture remains a passed preliminary observation; the final
fixture adds saved owner baselines and per-phase hash observations for audit.
No operation is replayed within either fixture. The first independent audit
starts before a separate legacy readback finishes and stops on incomplete
evidence; only that read-only audit is repeated after the original dependency
exits successfully. The retained failure is not relabeled as a successful run.
Final acceptance is `.validation/msi-major-upgrade-accepted.json`.

Hosted authoring and local native
installation are separate evidence. This does not close current-application
upgrade/rollback, EXE/updater `current.txt` migration, locked files, normal setup
UI/startup, approved model onboarding, second-PC, signing/Defender or stable
v0.4.0 acceptance. Automatic updates still reject prereleases using source SemVer.
