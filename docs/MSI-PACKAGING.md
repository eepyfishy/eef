# MSI packaging: requirements and current blockers

MSI remains an in-development release requirement, not a replacement already
available to users. The existing EXE installers and published artifacts stay
unchanged until replacement packages pass acceptance.
The [failed-upgrade probe](MSI-ROLLBACK-VALIDATION.md) now exposes an unresolved
registration rollback failure after actual writes on the standard-user host;
file restoration alone is not accepted. No production MSI is released.

The current [version encoding contract](MSI-VERSION-POLICY.md) is
`packed-patch-v1`. The planner/author enforce its source-to-numeric relation and
the independent inspector verifies it separately. Earlier arbitrary numeric
development examples below remain historical evidence, not production ordering.
Encoding does not by itself implement upgrade/downgrade fences or migration.
The separate [major-upgrade-v1 development recipe](MSI-UPGRADE-POLICY.md) now
authors and independently inspects role-scoped installed-version detection,
downgrade refusal and in-transaction older-product removal. Its guarded synthetic
lifecycle test is opt-in and separate from non-installing authoring/CI. Real
application migration/rollback and release acceptance remain open.

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
  and file inventories. Both roles bundle Python. eef requires no local LM or
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

This establishes feasibility, not an eef installer. The probe contains no eef,
eefn, Python or llama.cpp payload and is not distributable. It does not establish
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
`0.4.0-alpha.5` cache: 53 eef files / 34,773,699 bytes and 2,286 eefn files /
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

## Development-only native authoring

The [development author](../tools/build-development-msi.ps1) now reuses a verified
role bundle without rebuilding the application. It requires explicit development
consent, source/MSI versions and a project namespace. It regenerates the inventory,
compresses selected files with the installed Windows cabinet utilities, verifies
every extracted file and the embedded cabinet in bounded readback chunks, and
authors one file per stable component with scoped HKCU key paths. The current
upgrade recipe adds only the exact message-only Type 19 downgrade refusal to
standard MSI actions; no custom executable or script is authored. The separate
[inspector](../tools/inspect-development-msi.ps1) opens the database read-only and
checks its tables, references, default per-user directories, current-user registry
scope, media, sequences, development guard and cabinet hash.

```powershell
  .\tools\build-development-msi.ps1 -BundleDir PATH -Role eefn -ExpectedVersion SOURCE_VERSION -ProjectNamespace PROJECT_UUID -OutputDirectory NEW_DIRECTORY -DevelopmentOnly
.\tools\inspect-development-msi.ps1 -Package PATH_TO_DEVELOPMENT_MSI -ExpectedSha256 EXPECTED_HASH
node tools/test-msi-authoring.mjs
```

These are not release installers. The package name and directory identify a
development build, and the authored launch condition rejects ordinary installation
without an explicit development property. That guard prevents accidental use; it
is not publisher authentication or protection against an owner changing a package.
No custom executable actions, services, startup entries, automatic launch or
installation are performed by these tools. ASCII build paths are currently
required; end-user installation/path encoding still needs acceptance.

The initial synthetic Windows regression passes for both roles, with cabinet data larger
than one readback chunk, source preservation, regenerated inventory verification,
existing-output refusal, missing-consent refusal and wrong-hash refusal. Seven
mutated recipes per role were rejected: machine context, changed guard, HKLM
registry, changed directory scope, custom action table, duplicate component GUIDs
and altered UI action position. Mutations affect copies in generated test fixtures
only and are never installed. The earlier inspector variable-type failure and
primary-key mutation-helper failure remain retained as authoring failures.
The current upgrade regression expands refusal coverage to 24 recipes per role;
its separate standard-user synthetic lifecycle and independent read-only audit
pass all ten native operations with owner markers retained. See
[the exact scope and remaining gates](MSI-UPGRADE-POLICY.md#opt-in-lifecycle-regression).

A separate read-only audit verifies development packages built from the accepted
current `4ea7964` default-feature applications and verified cached runtimes:
54 files / 92,694,522 payload bytes for eef and 2,287 files / 332,015,564 bytes for
eefn. The resulting MSI files are 27,652,096 and 118,575,104 bytes respectively.
Current Python host/plugin assets and README files replace old application assets;
runtime provenance remains separate from application provenance. No application
build or model download was performed. Runtime files were hard-linked into owned
staging, not copied. Hash-verified verification copies and intermediate cabinets
were removed after success and can be regenerated; original bundles and final MSIs
remain. These packages have not been installed or launched, signed, submitted for
Defender clearance or distributed. Their temporary namespace and numeric version
are not approved production identities/version ordering.

The synthetic authoring CI step is Windows-only and performs no installation.
Hosted acceptance of this new authoring increment remains separate from the
accepted `4ea7964` run. Fresh-install application health, two-role installation,
startup choices, repair/uninstall, migration, rollback and release gates below
remain required.

## Current-application lifecycle checkpoint (2026-10-25; development only)

Published application/tool source `f9951af` passes all four GitHub jobs in
[run 40194073272](https://github.com/eepyfishy/eef/actions/runs/40194073272),
including the exact thirty native Windows suites, thirty-four planner cases,
two-role development authoring and lowercase naming checks. Both complete local
Rust modes pass 1,132 tests plus one existing ignored profiler each. Fresh
default-feature applications also pass the existing browser regression.

New guarded packages use those exact current applications with independently
verified cached runtimes; application and runtime provenance remain separate.
Independent read-only reconciliation recomputes the complete plans and component
identities and verifies the current application hashes, package hashes and new
inventories. These are different artifacts from the earlier `4ea7964` candidates.

One controlled standard-user test then installs both roles into their authored
default, distinct per-user directories. Every installed immutable file matches
its expected hash. No configuration is packaged. The node starts alone, creates
its stable ID/detected hostname and waits for a coordinator. Ordinary launches,
without configuration or port arguments and from an unrelated working directory,
create saved settings, automatically register and serve the lowercase dashboards.
Explicit private enrollment preserves identity, permissions and model choices.
Local consent followed by coordinator-owned permission changes, node restart,
bundled Python/node llama.cpp probes and cold relaunch all pass. The unrelated
working directory remains empty. No model is downloaded or inferred.

The first repair-preparation attempt exposes a **test-harness** cross-volume
`rename` error between the installed C: directory and retained E: evidence. It
occurs before either repair starts. The original controller and failure remain
recorded; the products/applications are not reinstalled or replayed. A separately
guarded continuation verifies existing payload/owner hashes and retired processes,
copies each package-owned test README into retained evidence, verifies both hashes,
and removes only that exact installed file before repair.

Both missing-file repairs restore the complete immutable payload with all saved
owner bytes unchanged. Both subsequent uninstalls remove all 54/2,287 package-owned
files and component registration values, unregister the products and preserve all
six generated owner files per role, including settings, stable node identity and
the unowned retention marker. Independent native registration/file/hash reads
verify final state. All six native transactions and their logs report success.
The invoking token is not administrator; no elevation, startup change, forced
Windows restart or security-policy change is requested. Test applications are
retired; retained settings/evidence and original recovery MSIs remain.

This proves one isolated, unattended current-application lifecycle on this PC,
not a non-coder release installer or physical second-PC acceptance. Start-menu
integration, installer dialogs/startup choices, same-package reinstall, Unicode
end-user paths, upgrade/downgrade ordering and migration, running/locked files,
real-application rollback, approved model onboarding, signing and Defender gates
remain open. Private enrollment was explicit, model readiness/quality was not
claimed, and the development guard and synthetic product namespace/version are
not approved production metadata. Published release installers are unchanged.

## Package contract

- Two independent per-user packages, eef and eefn, can coexist on one PC. Keep
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
- Enforce [packed-patch-v1](MSI-VERSION-POLICY.md) and verify actual installed-product
  upgrade/downgrade fences before producing distributable packages.
  [Windows Installer compares only three ProductVersion fields](https://learn.microsoft.com/en-us/windows/win32/msi/productversion);
  adding an alpha suffix/fourth field is not a safe ordering scheme. The encoding
  relation alone does not implement detection, removal, migration or recovery.
- Locked files/running roles, rollback, failed initialization and requested restart
  need truthful errors and explicit behavior; no success claim from extraction.
- Retain signing, SHA inventories and Defender investigation gates. MSI is neither
  an antivirus clearance nor an elevation bypass.

## Acceptance still required

Retain the separate table/component/CAB, synthetic rollback and current-payload
evidence above; do not substitute it for the remaining release checks.
Extend dedicated-directory first install and missing-file repair/uninstall to
release metadata, normal setup UI and additional machines. Also test reinstall, EXE migration,
MSI upgrade/downgrade fences, cancellation/rollback, locked-file behavior,
unattended installation, startup choices and uninstall with owner data intact.
Test both roles together and actual backend launch, stable node ID/hostname,
registration, permissions and model setup without JSON/port knowledge. Separate
physical second-PC evidence from extracted-package or isolated fixtures. Keep
existing installers until these checks, signing/Defender and release gates pass.
