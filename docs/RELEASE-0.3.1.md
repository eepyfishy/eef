# EEF v0.3.1

Windows installer maintenance release following the v0.3.0 EEFN Defender
detection. Both new installers passed local Defender scans with definitions
`1.459.63.0` and real-time protection enabled. The original v0.3.0 detection
remains unresolved; no Microsoft false-positive verdict has been received.
Do not restore the quarantined v0.3.0 file or add an antivirus exclusion.

Changes:

- Hash-locked Python runtime dependencies, pinned vendor checksums, and
  component/runtime/final-installer scan gates.
- Embedded dependency provenance and per-file SHA-256 inventories.
- EEFN ships the llama.cpp server and libraries without the extra upstream
  executable tools. Neither product ships or downloads a model.
- Installer version metadata and an explicit launch choice. Quiet installs
  do not launch without `--launch`; quiet upgrades preserve startup choices.
- Per-user, non-admin installation and the 5 GB installer storage floor remain.

Validation: 31 Rust tests passed; both installers passed isolated install and
upgrade checks, startup/config preservation checks, and installed-file hash
checks. Bundled Python media imports and llama.cpp startup checks passed.

These builds are still unsigned. A clean local scan is not a Microsoft
clearance or a guarantee about browser reputation/other antivirus engines.
Browser-download testing on the two PCs remains necessary. See the
[investigation and exact scope](https://github.com/eepyfishy/eef/blob/main/docs/ANTIVIRUS.md).

SHA-256:

```text
eef-installer.exe
97bfbbb927cedd487ff4ff8ab2774d185c943de2ece236f6d22beea5f8f383cb

eefn-installer.exe
be5cd41c31c7eea4867a9106c44127df2deec78d30203670f9696a3ca0bb0cbe
```
