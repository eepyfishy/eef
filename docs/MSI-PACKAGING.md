# MSI packaging: requirements and current blockers

MSI remains an unimplemented release requirement, not a replacement already
available to users. The existing EXE installers and published artifacts stay
unchanged until replacement packages pass acceptance.

## Initial inspection (2026-10-23)

The machine already has .NET SDK 10.0.100, supported .NET runtimes, Windows
Installer and MakeCab. At that inspection no WiX tool was installed and no MSI
had been built or installed. About 7.08 GiB was free. Reuse verified payloads rather
than downloading another Python/llama.cpp runtime or rebuilding the application.

The existing installer does more than extract files:

- `crates/installer/src/main.rs` preserves existing coordinator `config/` and
  node `config.json`, initializes a missing/default coordinator secret, and
  creates per-user startup/Start-menu entries with the correct working context.
- `select_manual_install` backs up a regular `current.txt` before selecting a
  manually installed root; it does not erase older version directories. Without
  equivalent migration, a new root could hand off to an older updater-selected
  application after an apparently successful MSI upgrade.
- `tools/bundle-windows.ps1` already produces separate role payloads, provenance
  and file inventories. Both roles bundle Python. EEF requires no local LM or
  llama.cpp; the node payload includes llama.cpp but no model weights.
  Signing/scan hooks and final artifact descriptor ordering must be preserved.

Simply wrapping the current self-extracting EXE in an MSI does not establish the
requested standard installation, repair, migration or uninstall behavior.

## Tooling decision needs owner approval

The current maintained WiX line has additional
[EULA/maintenance-fee terms](https://docs.firegiant.com/wix/osmf/), including an
explicit EULA acceptance mechanism in version 7. No terms have been accepted,
sponsorship started, account connected or package installed for this work.
The owner must approve that tooling agreement before it is accepted. Do not
infer the owner's eligibility or financial obligations from the project being
open source, and do not silently select an unsupported version to avoid the
decision. Windows Installer APIs are an alternative authoring route but need
their own maintainability and complete validation assessment.

This tooling decision does not block unrelated command/backend development.

## Native feasibility (2026-10-25; synthetic only)

A development-only Windows Installer API/MakeCab probe now authors a complete
compressed package with one 37-byte text file and one isolated HKCU component
key. Independent read-only checks verify the table relationships, payload/CAB
hashes, summary architecture, per-user directory, standard action sequence and
absence of custom executables, services, startup entries or removal globs.
The package has a distinct synthetic product identity and an explicit guard
against accidental installation. No WiX tooling, additional download or tooling
agreement was needed for this probe.

The controlled lifecycle test installed that exact package in its unique
per-user test directory, verified the current user's unmanaged installation
context, moved only the hash-checked synthetic payload to retained evidence,
and repaired the missing file. Uninstall removed the package-owned file and
registry key and unregistered the product. A generated unowned text marker
survived both repair and uninstall. All three native actions returned zero.
No elevation was requested and no security policy was changed.

A second distinct text-only package tests rollback after actual pending-script
execution. Its complete standard sequence includes
[`InstallExecute`](https://learn.microsoft.com/en-us/windows/win32/msi/installexecute-action)
before a deliberately failing
[message-only Type 19 action](https://learn.microsoft.com/en-us/windows/win32/msi/custom-action-type-19).
There is no custom executable or script. Independent package readback verifies
the original scope/payload, all twenty-one sequence rows, separate package
identity and explicit two-flag installation guard before the controlled test.
Native logs confirm file/registry operations and successful pending-script
execution before the expected error, then rollback removal operations and the
expected installer exit code 1603. Independent current-state reads confirm the
product is unregistered, owned payload and user key are absent, and the generated
owner-data marker remains intact. The intended native failure is a passing
rollback test, not a successful application installation.

This establishes feasibility, not an EEF installer. The probe contains no EEF,
EEFN, Python or llama.cpp payload and is not distributable. It does not establish
full ICE validation, upgrade/version ordering, real application rollback, locked files, EXE or
updater migration, two-role coexistence, startup choices, first-run application
health, signing or Defender clearance. Those gates below remain open. Existing
published installers are unchanged.

The [backend first-run increment](FIRST-RUN-INITIALIZATION.md) separately moves
missing configuration creation into the applications, with existing owner bytes
preserved. It removes reliance on an installer custom action for that creation;
it does not supply an MSI migration or repair transaction.

## Experimental read-only payload planning

The standalone [planner](../tools/msi-payload-plan.mjs) checks inventory-backed file/byte hashes, safe paths,
case aliases, selected links, explicit source/MSI versions and project-scoped role
identities before any CAB or MSI authoring. Mutable configuration is omitted
without reading it. Recognized CPython `__pycache__` entries are omitted only when
their matching source files are included and independently hash-verified;
orphan bytecode and other content in a cache directory are not silently dropped.
The transformed distribution needs a new inventory, not the original file list
that still names mutable configuration and generated caches.

Thirty-four isolated [test cases](../tools/test-msi-payload-plan.mjs) pass, including the RFC UUIDv5 vector, role/fork identity
separation, same-size corruption, path aliases, junction refusal and old-version
relabelling refusal, local-path-only enforcement and secret-free CLI errors.
Independent read-only checks also verify an existing
`0.4.0-alpha.5` cache: 53 EEF files / 34,773,699 bytes and 2,286 EEFN files /
273,958,002 bytes, excluding one configuration per role and 134 source-backed
node Python caches. The earlier filename/cache refusals remain recorded.
No cache contents or inventories were changed, copied or repackaged. Those old
payloads are not the current application source and were not relabelled as such.

```text
node tools/msi-payload-plan.mjs --help
node tools/msi-payload-plan.mjs --bundle PATH --role eefn --namespace PROJECT_UUID --expected-version PACKAGE_SEMVER --msi-version MAJOR.MINOR.BUILD
node tools/test-msi-payload-plan.mjs
```

All identity/version inputs are explicit. A project controls its stable namespace;
do not derive it from an account or host path, regenerate it per build, or reuse
another project's identity for a fork. The test namespace and numeric versions
are synthetic, not approved production metadata. Plans are JSON; failures return
nonzero with a bounded error code, not an absolute source path. No payloads,
mutable settings, inventories or installer registrations are changed.

This is experimental planning evidence, not an MSI builder or installer. It
does not approve a permanent product namespace, release-version ordering, a new
package identity, publisher trust, migration or current first-run compatibility.
The standalone public tool and its synthetic CI step are separate from the
accepted application-source checkpoint. Hosted follow-up and actual MSI authoring
remain required; application initialization compatibility is explicitly unverified
for the old cached payloads used here.

## Package contract

- Two independent per-user packages, EEF and EEFN, can coexist on one PC. Keep
  distinct stable upgrade/product identities and an explicit version policy.
  Build identity belongs to project metadata, never a contributor's account.
- Install into dedicated writable per-user directories. Preserve the existing
  Downloads-directory protection, optional startup and normal Start-menu launch;
  do not require permanent elevation or change UAC/Defender settings.
- Reuse role inventories and verified payloads. No embedded model/default change,
  hidden download, arbitrary custom installer wrapper or second application build.
- Preserve mutable settings, credentials, stable node identity, model files, job
  data, permissions and startup opt-outs across repair/upgrade. Uninstall removes
  only package-owned resources and never automatically deletes user data.
- Separate distribution templates from mutable first-run files. Initialization
  must use backend-owned validation/atomic writes and never overwrite a valid
  owner secret, require hand-editing JSON or leave an unusable placeholder secret.
- Migrate EXE installations and updater-selected generations deliberately. Backup
  prior selection for recovery, do not delete old versions as a side effect, and
  verify which application will actually launch after commit or rollback.
- Define prerelease/stable MSI version ordering before producing distributable
  packages. [Windows Installer compares only three ProductVersion fields](https://learn.microsoft.com/en-us/windows/win32/msi/productversion);
  adding an alpha suffix/fourth field is not a safe upgrade-ordering scheme.
- Locked files/running roles, rollback, failed initialization and requested restart
  need truthful errors and explicit behavior; no success claim from extraction.
- Retain signing, SHA inventories and Defender investigation gates. MSI is neither
  an antivirus clearance nor an elevation bypass.

## Acceptance still required

Validate MSI tables, component ownership and CAB contents before installation.
Then test dedicated-directory first install, reinstall, repair, EXE migration,
MSI upgrade/downgrade fences, cancellation/rollback, locked-file behavior,
unattended installation, startup choices and uninstall with owner data intact.
Test both roles together and actual backend launch, stable node ID/hostname,
registration, permissions and model setup without JSON/port knowledge. Separate
physical second-PC evidence from extracted-package or isolated fixtures. Keep
existing installers until these checks, signing/Defender and release gates pass.
