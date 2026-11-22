# Build, test, and bundle

## Toolchain

Windows builds use Rust's `x86_64-pc-windows-gnullvm` host and LLVM-MinGW. This
combination supplies the C compiler required by `ring` and bundled SQLite. The
Windows bundler explicitly selects the `x86_64-pc-windows-gnullvm` host. The
repository has no host-specific rustup override, so other platforms keep their
normal Rust host; the workspace declares its minimum Rust version in
`Cargo.toml`.

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

The EXE installer reads and verifies every archive entry before writing to the
destination. It checks replacement access to existing application files and
keeps those handles until extraction finishes. A file already locked against
replacement therefore stops setup before earlier files are changed. Existing
configuration is excluded from this replacement check.
An OS lock on `.eef-install.lock` serializes setup for each destination through
configuration, shortcuts and launch. A second setup for that folder asks the
user to wait. The lock file is retained for reuse and the OS releases the lock
when the setup process exits.
Setup also reads the existing `bundle.json` before extraction and requires the
installed role to match. Use separate destination folders for the coordinator
and node. Older manifests with a UTF-8 BOM and extra metadata remain compatible.
It then stages and synchronizes each payload file before replacing
the installed copy. Corrupt payloads fail before any installation write. A refused
file replacement leaves that file's previous bytes in place. Existing configuration is kept, including a configuration
created while setup is copying files. Whole-install rollback is a separate gate.
Setup also reports an unsuccessful app exit during its initial launch wait.
The command-script path checks for shortcuts run before payload extraction.
Sign-in and Start menu command files use the shared atomic writer, preserving
the previous shortcut if its replacement fails.

Pass `-ExpectedSourceCommit` with the full build commit when collecting
release evidence. The check requires both installed packages to record that
clean revision and the installed version, including after an upgrade. The
report records the tested installer hashes with the revision.

Useful switches:

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
