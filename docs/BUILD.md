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

The script downloads LLVM-MinGW 22.1.8 into the ignored `.tooling` directory
when needed, validates its SHA-256, runs `cargo test --workspace --all-targets`,
builds both release executables, and deletes Cargo output by default. It obtains
CPython 3.11.9 from python.org and verifies the SHA-256 published in Python's
Windows release manifest.

Useful switches:

- `-SkipTests` — reuse a test result from the same source revision.
- `-SkipOptionalPythonPackages` — omit Pillow/PyAutoGUI while retaining CPython
  and standard-library plugins.
- `-KeepBuildArtifacts` — retain `target` for development; this uses much more
  storage.
- `-KeepToolchain` — retain a compiler downloaded by the script.

The bundler requires 8 GB free at startup so its temporary build use cannot
cross the 5 GB safety floor. It checks the floor again before and after bundle
assembly, removes a partial output after failure, and refuses to overwrite an
existing output directory. The release directory contains exactly two
archives: one for EEF and one for EEFN. A combined ignored staging bundle is
retained only for local diagnostics.

## Validation performed for 0.2.0

```text
cargo check --workspace --all-targets  pass
cargo test --workspace --all-targets   23 passed, 0 failed
cargo fmt --all -- --check             pass
eef.exe --version                      0.2.0
eefn.exe --version                     0.2.0
bundled python -I                      CPython 3.11.9
extracted installers                   pass (separate dirs + opt-in startup)
live node/dashboard/failover smoke     pass
```
