# MSI version policy: packed-patch-v1

This is a packaging protocol for eef/eefn, not a dashboard preference, a release
approval or an installed-product migration. The application keeps its canonical
source version. Windows Installer receives a separate, deterministically encoded
numeric version. Forks retain explicit project namespaces; the mapping contains
no account, machine, repository or contributor identity.

## Encoding and supported range

[Windows Installer ProductVersion](https://learn.microsoft.com/en-us/windows/win32/msi/productversion)
compares three numeric fields, with major/minor at most 255 and build at most
65,535. A fourth field is ignored. eef's supported canonical source forms are
`major.minor.patch`, or that form followed by `-alpha.n`, `-beta.n` or `-rc.n`.

For this policy, source major, minor and patch are each 0–255. Prerelease sequence
numbers are 1–63. MSI major/minor equal source major/minor; MSI build equals
`256 * patch + ordinal`:

| Source phase | Ordinal |
| --- | --- |
| `alpha.n` | `n` (1–63) |
| `beta.n` | `64 + n` (65–127) |
| `rc.n` | `128 + n` (129–191) |
| Stable | 255 |

Thus `0.4.0-alpha.6` maps to `0.4.6`, `0.4.0` to `0.4.255`, and
`0.4.1-alpha.1` to `0.4.257`. The maximum supported stable source,
`255.255.255`, maps to `255.255.65535`. Gaps are reserved, never implicitly used
for another channel. Within the supported source grammar, numeric ordering
preserves [semantic version precedence](https://semver.org/spec/v2.0.0.html).

Leading zeros, whitespace, alternate labels, build metadata, zero/overflowing
prerelease counters and out-of-range components fail closed. The tool does not
wrap, clamp, discard a suffix, guess a different release label or allocate a
host-dependent build number. A release outside these limits needs a reviewed
new policy and installed-product migration, not an unnoticed mapping change.
The canonical application version remains visible in application status and
package source metadata; the encoded MSI number is not a replacement SemVer.

## Authoring and inspection

```text
node tools/msi-version-policy.mjs --source-version 0.4.0-alpha.6
node tools/msi-version-policy.mjs --source-version 0.4.0 --msi-version 0.4.255
node tools/test-msi-version-policy.mjs
```

The helper is read-only. It derives or verifies one numeric version; invalid
input produces a bounded code without echoing paths or arbitrary input.
The [payload planner](../tools/msi-payload-plan.mjs) requires an explicitly
matching numeric version. The [development author](../tools/build-development-msi.ps1)
derives that version when `-MsiVersion` is omitted; a supplied value must match.
It records `packed-patch-v1` in the private `eef_msi_version_policy` MSI property.
The lowercase marker is static package metadata, not a public installer option.
The existing
development-only installation guard remains; no release package is produced.

The [independent inspector](../tools/inspect-development-msi.ps1) implements the
bounds/mapping separately in PowerShell and rejects missing/unknown policy,
invalid source version or inconsistent numeric version before accepting the
recipe. It does not invoke Node to validate a package. Current author/inspector
reports expose `version_policy` and `version_policy_verified`.

Earlier experimental packages used arbitrary explicit numeric versions. Default
inspection now refuses their missing marker. Explicit read-only
`-AllowLegacyExperimentalVersion` permits diagnosis, reports
`legacy-explicit-development`, and leaves `version_policy_verified` false. This
does not convert, install, migrate or approve an old package. An unknown marker
is never accepted by that switch. Keep historical packages and evidence intact.

## Evidence and remaining gates

The regression enumerates all 48,640 supported patch/phase/sequence positions
for one major/minor pair, verifies unique increasing ordinals, checks major/minor
boundaries and explicit vectors, and rejects malformed/overflowing inputs. Its
13 cases run before the 35-case payload planner in every CI matrix job.
Native authoring checks both roles, six independent inspector boundary packages
and thirteen refused mutated recipes per role. The policy cases execute no payload
or MSI installation. Real installed-version detection/removal is separate.

`version_policy_verified` proves this encoding relation only.
`version_ordering_approved`, migration and release acceptance remain false until
the actual installer upgrade/downgrade, running/locked-file, rollback, EXE/updater
selection and recovery gates pass. Version encoding alone does not add
[upgrade-table detection](https://learn.microsoft.com/en-us/windows/win32/msi/upgrade-table)
or safely replace an existing product. Namespace/production identity approval,
normal installer UI/startup, approved model onboarding, second-PC and
signing/Defender requirements also remain open.

No updater feed, automatic-update preference or application selection is changed.
Prerelease blocking continues to use the canonical source version, not the
encoded MSI number; deriving a stable number never authorizes an update.
