# EEF 0.4.0a — opt-in Windows alpha

User-facing release/tag: **v0.4.0a**. Internal Rust/bundle version:
**0.4.0-alpha.1**. This intentionally ships the essential working slice before
the remaining roadmap. It is not the final v0.4.0 release.

## Install and test

Download the two Windows x86-64 installers from this prerelease:

- `eef-installer.exe`: the coordinator and its dashboard, with bundled Python.
- `eefn-installer.exe`: the node app, bundled Python/media adapters and llama.cpp.

Install both on your first PC and launch both from the Start menu. Use the node
dashboard for input, permissions, models and job history. Same-account local
pairing requires no JSON or port entry. A second PC can run EEFN alone and join
using a private connection code from EEF's Network page. EEF needs a connected
node; nodes do not each require a local EEF process.

No model is included or downloaded at installation. Choose an existing Ollama
model, request a model download, or select an existing GGUF file in Models.
Settings changes apply after restart; optional startup remains per-user.

Stop the old apps before upgrading and back up their configuration and EEF
database. Install into the existing dedicated app folders, not Downloads.
The installers preserve configuration, but this alpha has additive database
changes: rollback to an old application is not a tested database downgrade.
Use a separate test installation or your backup if you need a clean rollback.
Manual installation backs up an existing `current.txt` update selection as
`current.before-manual-install-*.txt` and selects the newly installed root app.
Existing version folders are retained; manual selection recovery is not an
automatic database downgrade.
Stable update feeds remain on v0.3.2; installing the alpha is an explicit choice.

## Included

- Compact node-first dashboard; background refresh preserves selection/focus.
- Local pairing, stable node IDs/hostnames, EEF reachability checks, persistent
  connection Stop/Resume, restart buttons and owner-approved remote configuration.
- Feature-wide permission switches with legacy scoped grants retained unless
  deliberately changed. No administrator/UAC bypass.
- Per-node Ollama/GGUF model selection, bounded download progress and cancellation,
  metadata inspection, hash checks where supported, and honest storage reporting.
- Immutable request origin, hierarchical areas and named resources, extending
  existing routing without claiming unsupported hardware or streaming behavior.
- Durable one-shot planned jobs: definitions, attempts, assignments, results,
  history, explicit Pause/Resume/Stop and origin-scoped node controls. Interrupted
  work never automatically replays. Completed steps remain completed; uncertain
  mutations refuse resume. Already-sent actions may still finish after Stop.
- Configurable job count/storage budgets with reserved recovery headroom. Saved
  settings survive page reload before restart; status shows applied settings.
- Existing permission-gated filesystem, process/app, HTTP, Wake-on-LAN and
  camera/audio/screen adapters remain available subject to hardware and drivers.

## Deferred / known limits

- Recurring jobs, live camera/screen/audio relay, RTSP workflows, detection and
  microphone correlation; chat-only and direct capability calls are not durable jobs.
- Wi-Fi/Bluetooth controls, authenticated peer mesh, ESP-NOW/multi-hop, distributed
  optimization, live migration, replicated coordinator state/election/fencing.
- Endpoint priorities are reconnect preferences, not safe active/standby clustering.
- Ollama's actual server disk space is not verified; the UI reports it as unknown.
- Physical two-PC and real media/radio acceptance remain outstanding. Automated
  one-PC tests and mock model downloads do not establish those claims.

## Security and validation

These builds are **unsigned**. Signing hooks exist, but no public certificate or
signing account is configured. The earlier second-PC Sabsik/Wacatac detections
against v0.3.2 remain unresolved. Passing local Defender scans cannot guarantee
acceptance on another PC and are not a Microsoft false-positive verdict.
Do not disable Defender, add exclusions, or restore a detected file to test it.

Release packaging requires locked Rust tests, verified third-party downloads,
component/bundle/final-installer Defender scans, and per-file hash inventories.
No AI models or personal credentials are included. See
[antivirus guidance](https://github.com/eepyfishy/eef/blob/v0.4.0a/docs/ANTIVIRUS.md),
[signing](https://github.com/eepyfishy/eef/blob/v0.4.0a/docs/SIGNING.md), and the
[full roadmap](https://github.com/eepyfishy/eef/blob/v0.4.0a/docs/ROADMAP-0.4.0.md).

### Build and test evidence (2026-09-07)

Both installers were built from clean source commit
`0a86da697ae1727c665957ab75f8867f85346770`, using locked dependencies and
Rust 1.98.1. Each installation includes its build provenance and file inventory.
The release-tag documentation commit records the evidence; it does not change
the packaged application source.

- All 84 Rust tests passed.
- Both graphical installer wizards passed installation through Finish, with
  isolated startup locations and launch disabled for the test.
- Both final installers passed fresh/quiet upgrade tests: configuration and
  startup choices preserved, explicit startup removal, old update selection
  backed up, prior version files retained, installed inventory hashes verified,
  and executable versions checked. Bundled node Python imports and llama.cpp
  startup passed; the fresh node contains no selected model or endpoint.
- Dashboard initialization and refresh/copy-stability regression passed.
- First-run/browser integration passed against the optimized bundled binaries:
  local pairing, stable identity, permissions, owner-approved remote management,
  node and coordinator restarts, browser/CLI input, and mock model operations.
- Durable job results survived a verified runtime restart; origin-scoped access,
  copy/details preservation, and saved/applied storage limits passed.
- Component, runtime archive, unpacked bundle, and both final installer scans
  passed with Defender engine `1.1.26080.3`, definitions `1.459.93.0`, and real-time
  protection enabled. These are local results, not cross-PC clearance.

### Final downloads

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| `eef-installer.exe` | 17,597,677 | `b50cfc8df0aaa488a4a0a6281d83286bec9f7f0e2020e8d1379370eee72f79ee` |
| `eefn-installer.exe` | 109,361,434 | `9e33a16f9033666bcb0ee29ef377bd5032442cb636151079a26e3398cc3258f7` |
