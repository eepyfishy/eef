# MSI packaging: requirements and current blockers

MSI remains an unimplemented release requirement, not a replacement already
available to users. The existing EXE installers and published artifacts stay
unchanged until replacement packages pass acceptance.

## Current inspection (2026-10-23)

The machine already has .NET SDK 10.0.100, supported .NET runtimes, Windows
Installer and MakeCab. No WiX tool is installed and no MSI has been built or
installed. About 7.08 GiB was free at inspection. Reuse verified payloads rather
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
