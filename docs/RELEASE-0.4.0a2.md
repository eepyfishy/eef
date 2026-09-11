# EEF v0.4.0a2 - two-PC diagnostic alpha

Internal version: `0.4.0-alpha.2`. This opt-in Windows x86-64 prerelease extends
v0.4.0a for physical two-PC development/testing. It is not the completed roadmap.
Stable update feeds remain on v0.3.2; installing this alpha is an explicit choice.

## Install

Download `eef-installer.exe` and `eefn-installer.exe`. Install both on the first
PC; install EEFN on the second PC. A node does not require local EEF. EEF can also
be installed on the second PC for a separate coordinator test, but do not run
the same stateful work on two coordinators: no safe cluster replication/fencing.

Close old apps and back up configuration/database before upgrading. Use the
existing dedicated directories, not Downloads. Config, stable node IDs and old
version directories are preserved; manual upgrades back up stale `current.txt`
selection so it cannot silently launch an older version. Database downgrade is
not tested. Startup remains optional and per-user. This release uses the tested
EXE installers; MSI packaging remains pending.

No model is included/downloaded automatically. Default lightweight-LM bootstrap
is on the revised roadmap, not implemented here. Existing Ollama/GGUF selection
and permissions continue to work.

## Added since v0.4.0a

- Shared node runtime services, not dashboard-owned model/configuration state.
- `eefn network show/set`, `--no-ui`, separate stable identity/display name/
  advertised address, and optional coordinator advertisements.
- `eefn network peers`: EEF-owner-scoped discovery with self-only defaults,
  bounded sanitized pages, heartbeat freshness and stale-address removal.
- `eefn network diagnose --json` and `eef diagnostics --json`: explicit minimal
  local reports with versions/runtime IDs and connection information/counters.
- `eef diagnostics --node NODE_ID --samples 5 --json`: bounded authenticated
  ping/latency/version tests, with failure counts. No shell, camera or model use.
- `eef invite --address HOST:PORT --json`: private pairing code with the actual
  coordinator overlay/LAN endpoint rather than relying on hostname resolution.

Reports do not automatically upload anywhere. Stable node/runtime IDs support
correlation; the diagnostic projections omit names, addresses, local paths,
secrets, prompts and raw errors. Pairing codes contain the network secret: never
attach them, configuration files or unreviewed logs to public issues.

Use the [two-PC guide](https://github.com/eepyfishy/eef/blob/v0.4.0a2/docs/TWO_PC_TEST.md)
and [command reference](https://github.com/eepyfishy/eef/blob/v0.4.0a2/docs/COMMANDS.md).
No test PC address, credentials or special developer access is embedded.

## Security and limits

Unsigned builds; no signing certificate is configured. The earlier second-PC
Sabsik/Wacatac reports remain unresolved. Local Defender passes are not Microsoft
clearance or assurance of acceptance on another PC. Do not disable protection,
add exclusions or restore detected installers to continue testing.

Keep dashboards local. Nodes connect to the coordinator gateway using private
pairing, and remote configuration retains owner approval. Discovery does not
authorize direct connections or execution. Shared-PSK authentication does not
isolate mutually untrusted key holders. Independently authenticated peers and
execution leases remain prerequisites for future direct data paths.

Default LM/interpreter, generalized model lifecycle, continuous media/workloads,
direct peer streams, mesh, coordinator clustering and MSI remain pending. Automated
two-node tests on one PC do not prove physical two-PC/overlay connectivity; that
acceptance takes place after both PCs install this build. A successful ping does
not establish inference, media or stateful-job correctness.

## Build and validation evidence

Built from clean source commit
`ecbb9ec1758b3b385297b6e9211e1fe5bf6d498f` (`source_dirty: false`). The release
tag includes a subsequent documentation-only evidence commit. Installed packages
contain `provenance.json` and `files.sha256.json`; third-party download hashes
were checked during packaging. The prerelease has exactly two installer assets.

Validated on 2026-09-11:

- 98 locked Rust workspace tests passed, followed by optimized Windows builds.
- Final bundled binaries passed command/diagnostic/private-invite integration,
  scoped discovery with two logical nodes, and first-run/browser integration.
- Dashboard initialization and copy-stability regression passed.
- Both installers passed isolated install/upgrade/config-preservation, startup
  opt-in/out and stale-version-selection backup tests. Installed inventory hashes
  and versions matched; bundled Python media imports and llama-server startup passed.
- Both native installer wizards completed through Finish, with no preview opened.
- Local Defender scans passed for first-party binaries, runtime archives, the
  unpacked bundle and both final installers. Engine `1.1.26080.3`; packaging
  definitions `1.459.151.0`; final installer retests `1.459.154.0`. Real-time
  protection remained enabled. These are local scan results, not Microsoft review.

Local evidence is retained under `.validation/`: `v0.4.0-alpha.2`,
`node-commands-uqSQ9y`, `peer-discovery-Y4yl4B`, `first-run-Fizikl`,
`install-test-e146c7f6b50c4ddd8611791ca6954491` and
`wizard-test-e022d1a3897f4e628f8c30685c0ef3d8`. Fixtures and raw reports are not
release assets. Physical two-PC acceptance is still pending.

| Installer | Bytes | SHA-256 |
| --- | ---: | --- |
| `eef-installer.exe` | 17,689,081 | `3d5c14c0a946ddc3fe02987acf254e16dd222cbfba0213d1f2f6f3702e11aa2a` |
| `eefn-installer.exe` | 109,453,396 | `b66ed4d9a3d7d6ee4c6031a7d62bea18f7ee46eca954168dea37dbdd1fb6b3a5` |
