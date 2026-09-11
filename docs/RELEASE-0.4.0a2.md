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

Final build provenance, hashes and validation evidence are recorded below after
packaging. The prerelease has exactly two downloadable installers.
