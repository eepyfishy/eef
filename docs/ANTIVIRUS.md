# Windows antivirus investigation

## Reported v0.3.0 detection

Microsoft Defender detected the published `eefn-installer.exe` as
`Trojan:Win32/Wacatac.B!ml` on 2026-09-06 (local time). It quarantined both a
browser download and the original local release file. Defender reported that
the detected threat had not executed and was no longer active.

The published artifact SHA-256 was:

```text
8d251f5ff0331069e08c1ab67e77a928f04034c44caadd9f96339c2e47622e6c
```

This is an antivirus detection, not just an unsigned-publisher/SmartScreen
warning. Do not restore this artifact or add an antivirus exclusion to test it.
Successful Rust tests, matching hashes, or an unsigned executable alone cannot
establish whether a detection is a false positive.

The original file is unavailable to further local inspection because it was
quarantined. Microsoft has not issued a false-positive determination, and no
sample has been submitted on the user's behalf. Developer review is available
through [Microsoft's submission portal](https://www.microsoft.com/en-us/wdsi/filesubmission).

## Investigation and maintenance changes

- The original EEF coordinator installer, Rust source, and Python adapter
  source passed explicit local Defender scans with definitions `1.459.63.0`.
  This does not clear the original EEFN installer or identify the exact trigger.
- Compiler, CPython, and llama.cpp downloads retain pinned SHA-256 verification.
- Windows Python runtime dependencies now have a complete version/hash lock,
  including transitive packages, checked against PyPI distribution metadata.
  Packages are fetched from the explicit PyPI index. Source-distribution build
  dependencies are still supplied by pip's isolated build environment.
- The bundler runs Defender scans on Rust executable components,
  vendor archives, assembled runtime, and both final installers. A failed scan,
  unavailable antivirus, or disappearance of scanned files blocks the build.
  Broad compiler-toolchain scans were stopped without a verdict after stalling;
  they are not reported as clean. Build tools retain pinned checksum checks.
- Each installer contains `provenance.json` with dependency origins and
  `files.sha256.json` with per-file sizes and hashes. These aid inspection;
  they are not a substitute for a publisher signature.
- EEFN packages only the llama.cpp server and its runtime libraries/licenses,
  rather than unrelated upstream tools.
- The installer identifies itself in Windows file properties and asks before
  launching the installed program. Quiet installs do not launch unless
  `--launch` is specified; unspecified startup settings are preserved.

## Reproduce release scanning

```powershell
.\tools\scan-windows.ps1 -Path .\release\v0.3.1\eefn-installer.exe `
  -ReportPath .\.validation\v0.3.1\eefn-installer.exe.scan.json
```

The script uses Microsoft's custom-scan `-DisableRemediation` option. This
does not disable Defender or alter antivirus configuration: it scans archives,
ignores exclusions, and prevents a quarantined detection from being counted as
a successful clean scan. Real-time protection remains enabled. See
[Microsoft's scanner documentation](https://learn.microsoft.com/en-us/defender-endpoint/command-line-arguments-microsoft-defender-antivirus).

Reports under `.validation/` identify the engine/signature versions and scope.
A clean scan records one engine's result at one point in time; a browser's
download/reputation assessment or another antivirus may differ. Published
installers must also be tested through the normal browser download path.

## v0.3.1 validation

On 2026-09-06 (local time), both final v0.3.1 installers passed explicit
Defender scans with engine `1.1.26080.3`, definitions `1.459.63.0`, and real-time
protection enabled. Both files remained present. Rust executable components,
CPython and llama.cpp archives, and the unpacked runtime also passed.

All 31 Rust tests passed. Isolated installer tests passed for both products:
quiet install without launching, opt-in startup, startup preservation on
upgrade, explicit startup removal, config preservation, and installed file
hash verification. Installed EEF/EEFN reported 0.3.1; bundled Python media
imports and the minimal llama.cpp server's version check passed. No models
or coordinator endpoints were preselected in the fresh node configuration.

The tested artifacts are:

```text
eef-installer.exe (17,324,064 bytes)
97bfbbb927cedd487ff4ff8ab2774d185c943de2ece236f6d22beea5f8f383cb
eefn-installer.exe (108,982,555 bytes)
be5cd41c31c7eea4867a9106c44127df2deec78d30203670f9696a3ca0bb0cbe
```

Their provenance records the v0.3.0 parent commit and `source_dirty: true`:
the v0.3.1 worktree was built and validated before committing release results.
This is not a bit-for-bit reproducibility claim.

Normal browser downloads on the two test PCs still need validation. No
Microsoft false-positive verdict has been received for the original v0.3.0
artifact. These results do not identify its detection trigger or prove it was
incorrectly classified.
