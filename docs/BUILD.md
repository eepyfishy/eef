# Build, test, and bundle

## Toolchain

### Hosted compilation for low-storage development

The `Windows application build` workflow runs on `validation/packages/**` pushes
or by manual dispatch. It compiles the default-feature release applications and
the static installer stub using pinned Rust `1.98.1` and the production bundler's
hash-verified LLVM-MinGW download. It checks Windows product metadata, executable
versions with only packaged runtime DLLs available, and the stub's compiler DLL
imports. Build output discovery follows Cargo metadata.

The three-day artifact contains only those executables, required compiler runtime
DLLs and `application-build.json`, which records source, toolchain and file
digests. It excludes the compiler cache and Python/model downloads. This avoids
compilation on a low-storage development PC. Verify the workflow's exact source
commit and artifact digest before using its contents. The manifest is a build
record, not an independent authenticity or security guarantee.

These are unpackaged, unscanned development files. The separate core verification
matrix must pass at the same source. Normal installer assembly, Defender scanning,
signing when required, package regressions and physical acceptance remain
necessary. This workflow does not publish releases or change update feeds.

### Local compilation

Windows builds use Rust's `x86_64-pc-windows-gnullvm` host and LLVM-MinGW. This
combination supplies the C compiler required by `ring` and bundled SQLite. The
Windows bundler explicitly selects the `x86_64-pc-windows-gnullvm` host. The
repository has no host-specific rustup override, so other platforms keep their
normal Rust host; the workspace declares its minimum Rust version in
`Cargo.toml`.

For normal host builds, the bundler obtains the release output directory from
Cargo metadata. `CARGO_TARGET_DIR` and Cargo's `build.target-dir` configuration
therefore apply to metadata checks, signing, scanning, application copies and
the installer stub. This allows compiler output on another drive without
accidentally using older binaries in the checkout's `target` directory. Output
discovery fails on invalid metadata instead of guessing a fallback path.
The nine checks in `tools/test-bundle-build-paths.ps1` exercise real Cargo path
resolution plus rejected metadata; they compile and package nothing.

The easiest reproducible build is:

```powershell
cd path\to\eef-rust
.\tools\bundle-windows.ps1
```

Install the `stable-x86_64-pc-windows-gnullvm` Rust toolchain and `rustfmt` first.
The script uses the installed toolchain without silently upgrading it. It
downloads LLVM-MinGW 22.1.8 into the ignored `.tooling` directory
when needed, validates its SHA-256, runs `cargo test --workspace --all-targets`,
builds eef, eefn, and the shared installer stub, then deletes Cargo output by
default. It obtains
CPython 3.11.9 from python.org and verifies the SHA-256 published in Python's
Windows release manifest. Rust dependency resolution uses `--locked`; Python
runtime dependencies use `python/requirements-windows.lock` with
`--require-hashes` and the explicit PyPI index. A CPython 3.11 x64 build host
is required for those Windows wheel hashes.

Microsoft Defender must be enabled. The build scans the runtime
archives, Rust binaries, unpacked bundle, and final installers before reporting
success. Reports are retained in ignored `.validation/v<version>/`. Every
installer also includes dependency provenance and a per-file hash inventory.
See [ANTIVIRUS.md](ANTIVIRUS.md) for the detection investigation and scan limits.

Before creating either installer, the bundler checks the staged eef and eefn
executables and their bundled Python at the expected versions without the build
toolchain on the search path. It also checks optional media imports in the node
package when those modules are included. A failed launch, unexpected version,
or failed import stops packaging.

After building, run the installer regression checks in an isolated test
directory. They use a temporary `APPDATA`, leaving personal startup entries
alone:

```powershell
.\tools\test-windows-installers.ps1 -ReleaseDir .\release\v0.3.2
```

Use `-PreviousReleaseDir PATH` with the matching pair of installers from a
different, previously approved release to also test a real in-place upgrade.
This case scans the prior installers and records the before and after installer
hashes. It checks that upgrade preserves configuration and both startup choices
across the two roles and installs the same application bytes as a fresh setup.
Full inventory checks run after both fresh installation and upgrade, excluding
the role's documented mutable configuration. Upgraded nodes also repeat bundled
Python media imports and the llama.cpp version probe. The shared checker has
synthetic tamper, traversal and empty-inventory regressions in
`tools/test-installer-package-checks.ps1`; importing its helper starts no process.
Use `-ScratchBase PATH` with an existing directory on another drive when the
workspace has insufficient room for full installed runtimes and upgrade backups.
The default remains the workspace's ignored `.validation` directory.

To check process-interruption recovery with full packages, use distinct approved
old and candidate installers and the expected candidate application digest:

```powershell
.\tools\test-installer-interruption.ps1 -ReleaseDir PATH -PreviousReleaseDir OLD_PATH -ScratchBase EXISTING_DIRECTORY -Product eefn -ExpectedBinarySha256 $expectedSha256
```

Repeat with `-Product eef` and that application's digest. The test installs into
a new isolated directory, stops only its own candidate installer after detecting
an executable replacement, and requires the expected executable bytes and a
valid uncommitted journal after termination. It then runs the normal installer
again, verifies settings and unrelated data preservation, checks all immutable
inventory entries, and probes the node's bundled runtimes. A completed setup or
missed interruption is a failed observation, never a recovery pass. It uses no
production fault hook and is separate from the small synthetic CI fixtures.
See [the full-payload recovery evidence](EXE-SETUP-ROLLBACK.md#full-payload-development-probe-2026-11-26)
for source and acceptance limits.

The EXE installer reads and verifies every archive entry before writing to the
destination. It checks replacement access to existing application files and
keeps those handles until extraction finishes. A file already locked against
replacement therefore stops setup before earlier files are changed. Existing
configuration is excluded from this replacement check.
The existing `.update.lock` serializes setup, updates, rollback and package
repair for each destination. Setup holds it through configuration, shortcuts,
commit or rollback, and an optional command-line launch check. The wizard's
Finish action opens the committed installation separately. A conflicting
operation asks the user to wait. The lock file is
retained for reuse and the OS releases the lock when the process exits.
Setup also reads the existing `bundle.json` before extraction and requires the
installed role to match. Use separate destination folders for the coordinator
and node. Older manifests with a UTF-8 BOM and extra metadata remain compatible.
It stages and synchronizes each payload file before publishing the installed
copy. Corrupt payloads fail before any installation write. Development setup
now moves old files into unique backups on their own drive and restores them
if a later setup step fails. This covers payloads, settings preparation, update
selection and shortcuts. Existing configuration is kept, including a
configuration created while setup is copying files. Development setup also
journals changes for retry after process termination. Power-loss durability and
full release acceptance remain separate gates. See
[EXE setup rollback](EXE-SETUP-ROLLBACK.md) for the exact limits.
Setup also reports an unsuccessful app exit during its initial launch wait.
The wizard keeps a failed launch on an explicit recovery page with
**Retry opening** and **Close**. Retrying opens the installed app again without
reinstalling files or changing settings. Setup never retries a launch on its own.
The command-script path checks for shortcuts run before payload extraction.
Sign-in and Start menu command files participate in the same setup transaction.

Pass `-ExpectedSourceCommit` with the full build commit when collecting
release evidence. The check requires both installed packages to record that
clean revision and the installed version, including after an upgrade. The
report records the tested installer hashes with the revision.

Useful switches:

- `-StagingBase C:\build-scratch` — use an existing directory on another drive
  for temporary role payloads and ZIPs. Each build creates a unique owned child;
  cleanup removes that child, never the supplied base. Cleanup refuses changed
  ownership markers and reparse points. The default remains workspace `.package`,
  which must not already exist. Bundle and release outputs still stay inside the
  workspace; this switch does not move compiler outputs (`CARGO_TARGET_DIR`).
- `-SkipTests` — reuse a test result from the same source revision.
- `-SkipOptionalPythonPackages` — omit Pillow, PyAutoGUI, audio, TTS, and camera
  packages while retaining CPython and standard-library plugins.
- `-KeepBuildArtifacts` — retain `target` for development; this uses much more
  storage.
- `-KeepBundle` retains the unpacked staging bundle for diagnostics.
- `-KeepToolchain` — retain a compiler downloaded by the script.

There is no reserved free-space floor in 0.3.2. Actual compiler usage can vary;
monitor free space on constrained machines. Installers and model downloads
check their payload's required space. The bundler removes a partial output
after failure and refuses to overwrite an
existing output directory. The release directory contains exactly two
self-extracting installers: one for eef and one for eefn. A combined ignored
staging bundle is removed by default; use `-KeepBundle` for local diagnostics.

## alpha.8 setup verification

Clean source `f74943bc77b58410779501354f2ae4baad4498d8` produces separate alpha.8
role installers with the complete optional Python/runtime payloads. Both pass
fresh and alpha.6 upgrades, inventories, interrupted setup with startup refusal
and recovery, and the full native wizard. Local Defender definitions
`1.463.488.0` pass both exact files. Temporary role staging used the owned external
directory option above; its child was removed while the supplied base remained.
See [installer hashes and verification scope](RELEASE-0.4.0a8.md).
Physical older-node connectivity is recorded separately from matching-candidate
two-PC acceptance in [the physical procedure](TWO_PC_TEST.md).

## Published alpha.7 and subsequent setup verification

[v0.4.0a7](https://github.com/eepyfishy/eef/releases/tag/v0.4.0a7) was built from
clean source `af4dcf495bd53196e3f423ccd0c9b1c341bda754`. Its two installer hashes
match the published asset digests. Fresh installation, upgrades from the published
alpha.6, the full wizard, and private local enrollment passed on the build PC.
Local Defender definitions `1.463.390.0` passed both exact files. Physical-node
upgrade acceptance remains pending.

Subsequent source `c207d4948f37eb36f4f8222202d892e42c4e4d11` passed
[all four CI jobs](https://github.com/eepyfishy/eef/actions/runs/44875180408),
including all 39 named Windows native regression scripts and the Windows app
metadata check. This source adds per-file atomic setup replacement and comparison
of release assets with Defender scan hashes. A locally rebuilt setup executable
also passed fresh installation and real alpha.6 upgrades using the unchanged
alpha.7 payloads. These mixed-source test files are local fixtures.

The earlier metadata validation run failed one interpreter answer test, then
passed on its second attempt at the same source. The cause remains unproven.
The answer test now reports retained receipt state, error code, persistence and
model call count on a failed workflow to aid diagnosis. These are test diagnostics.

## Local command-first installer validation (2026-11-21)

The isolated Windows build used clean revision
`e975866996db5247cebdadec6d92c49f63b5f048` and the development version
`0.4.0-alpha.6`. The build passed 1,882 local Rust tests with one ignored.
Staged startup, embedded Python, optional media imports, and the installed
llama.cpp startup probe passed. Defender scanned both final installers with
engine `1.1.26100.4` and definitions `1.463.390.0`.

| Local installer | Bytes | SHA-256 |
| --- | ---: | --- |
| `eef-installer.exe` | 23,494,930 | `07b9ba9202d52fd9b37bdd258c3ed1e49dea6561d92ff2e99f4cb5a01d2f9e51` |
| `eefn-installer.exe` | 114,903,299 | `637ef50d93d88dbd9c2a31ce8823180d5553f49ff78e3b7b6f3fc77182269702` |

Fresh installs and real upgrades from the published alpha.5 installers
preserved configuration and startup choices in isolated directories. The
fresh and upgraded executables matched, and both recorded the expected clean
revision. The model-free private pairing fixture then used the installed
executables. Both logical nodes confirmed registration over pinned TLS through
`eefn setup --wait-registration-seconds 30`. The fixture also covered explicit
restart consent, saved trust reuse, revocation, and lost replies without replay.

Evidence is retained in the local installer and pairing reports. This
checkpoint covers one Windows PC. Physical two-PC operation and Authenticode
verification remain open.

## First-run browser regression (0.3.2)

`tools/test-first-run.mjs` launches isolated eef and eefn processes, with random
ports and temporary configuration/discovery/startup directories. It tests
stable identity, same-account pairing, local approval before remote changes,
both restart paths, browser forms, and a narrow-screen layout. A mock Ollama
service covers explicit install/select/registration and stalled-download
cancellation; it does not prove real inference or touch personal Ollama models.

Build debug binaries first. Install the test-only dependency with
`npm install --prefix .tooling/ui-tests --ignore-scripts --save-exact playwright@1.63.0`,
then run `node tools/test-first-run.mjs`. It uses installed Chrome, or the
browser path in `EEF_TEST_BROWSER`; no browser is downloaded.
Reports and screenshots stay in ignored `.validation/first-run-*` directories.

`tools/test-installer-ui.ps1 -ReleaseDir PATH` exercises the native wizard with
isolated startup settings. It checks that Back retains options and Review
precedes file writes. The report records each tested installer hash alongside
the Defender scans. Use `-LayoutOnly` to stop at Review and verify that
cancelling leaves the destination untouched.

After building the installer and its `installer_launch_fixture` example, run
`tools/test-installer-launch-ui.ps1 -BinaryDir target/debug` to check launch
recovery with small synthetic payloads. Both roles cover closing after a failed
launch, another explicit failed retry, and a successful retry. The check uses
isolated Start menu/settings paths, scans its two test installers with Defender,
records binary hashes and confirms that retries preserve installed files.
It also checks seven invalid folder choices per role on the Options page,
before Review or installation writes. The test driver exchanges
`WM_GETTEXT`/`WM_SETTEXT` with the other process's controls, as required by the
[Windows control-text API](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowtextw).
It does not launch the actual coordinator or node, verify app health, or replace
full-package installation and physical two-PC acceptance.

Development installers share read-only folder checks between the wizard and
quiet command-line setup. Empty choices stay on Options; drive roots, files,
unsupported shortcut-path characters, the other role's package, and locations
overlapping the downloaded installer are rejected before Review. Existing
ancestors are resolved before checking overlap, including Windows canonical
prefixes and case variants. No installation folder or lock is created by these
checks. Installation still checks disk space and rechecks the role under the
setup/update lock before extraction. Folder checks do not prove rollback or
crash recovery.

Local validation of this increment passed 39 installer Rust tests, 14 native
invalid-folder cases, four launch-recovery cases, and both roles' Back/review/
cancel checks. The native tests used small synthetic payloads, isolated settings
and Start menu paths, and Defender definitions `1.463.429.0`. They are development
evidence, not certification of a newly published package.

The [real CPU inference test](CLIENT-ACCEPTANCE.md#running-the-checks) uses an
unpacked runtime bundle and an explicit catalog candidate. You can reuse a
matching artifact or separately opt in to downloading that candidate into the
test's ignored validation directory. The test selects the candidate in its own
configuration and invokes inference through eef.

## Historical validation performed for 0.3.1

For current results, see [v0.3.2 release validation](RELEASE-0.3.2.md).

```text
cargo test --workspace --all-targets --locked   31 passed, 0 failed
cargo fmt --all -- --check                     pass
PowerShell script syntax                      pass
component, runtime, and final Defender scans   pass (definitions 1.459.63.0)
isolated install and upgrade regression        pass (eef and eefn)
installed file inventory checks                pass
bundled Python media imports                   pass
minimal llama.cpp server --version             pass (build 10621)
```

See [ANTIVIRUS.md](ANTIVIRUS.md) for exact artifact hashes and remaining
browser-download/Microsoft-review limitations. The two-PC live failover test
below was performed for 0.3.0, not repeated for this maintenance build.

## Historical validation performed for 0.3.0

```text
cargo check --workspace --all-targets  pass
cargo test --workspace --all-targets   31 passed, 0 failed
cargo fmt --all -- --check             pass
eef.exe --version                      0.3.0
eefn.exe --version                     0.3.0
bundled python -I                      CPython 3.11.9
media package import                    pass
self-extracting installer smoke         pass (separate dirs + config creation)
live node/dashboard/failover smoke     pass
```
