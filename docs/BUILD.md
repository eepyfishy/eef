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
builds EEF, EEFN, and the shared installer stub, then deletes Cargo output by
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

After building, run the installer regression checks in an isolated test
directory. They use a temporary `APPDATA`, leaving personal startup entries
alone:

```powershell
.\tools\test-windows-installers.ps1 -ReleaseDir .\release\v0.3.2
```

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
self-extracting installers: one for EEF and one for EEFN. A combined ignored
staging bundle is removed by default; use `-KeepBundle` for local diagnostics.

## First-run browser regression (0.3.2)

`tools/test-first-run.mjs` launches isolated EEF and EEFN processes, with random
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

`tools/test-installer-ui.ps1 -ReleaseDir .\release\v0.3.2` exercises each
native setup window with isolated startup settings, folder choice, Install,
and Finish. It retains welcome/completion screenshots and scan reports.

For an **explicit real-model download test**, retain the unpacked bundle and
run `node tools/test-real-model.mjs bundle/eef-windows-x86_64-v0.3.2`.
This downloads the catalog's approximately 491 MB starter model into a unique
ignored validation directory, selects it, waits for the bundled CPU runtime,
and invokes inference through EEF. It does not use personal model storage.
Remove that exact generated test directory afterward if space is limited.

## Historical validation performed for 0.3.1

```text
cargo test --workspace --all-targets --locked   31 passed, 0 failed
cargo fmt --all -- --check                     pass
PowerShell script syntax                      pass
component, runtime, and final Defender scans   pass (definitions 1.459.63.0)
isolated install and upgrade regression        pass (EEF and EEFN)
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
