# EEF v0.4.0a3 — command-first backend alpha

Internal version: `0.4.0-alpha.3`. Opt-in Windows x86-64 prerelease; not the
completed roadmap. Two EXE installers: EEF coordinator and independent EEFN node.
MSI, default LM bootstrap, interpretation, execution leases, continuous workloads,
direct peer media and coordinator clustering remain pending.

## Updates and installation

Automatic updates remain stable-only. Default manifests remain on v0.3.2;
this alpha does not replace them. Both roles reject all prerelease feed updates
at check and install time, even if an update feed accidentally points to an
alpha/beta/RC. A future stable v0.4.0 can supersede this alpha normally.
Choose prerelease installers explicitly; no automatic alpha subscription.

Close the apps before a manual installer upgrade. Use their existing dedicated
installation directories. Configuration, stable IDs, data, previous version
directories and existing startup choices are retained by a quiet upgrade.
Back up configuration/database first; database downgrade is not tested.
No model is bundled or automatically downloaded. Ollama and GGUF remain owner-selected.

## Added since v0.4.0a2

- Optional browser builds and `--no-ui` for both roles. Commands, runtime state,
  models, jobs and connection control do not require a dashboard.
- Node connection show/pause/resume/pair-local, actual local API discovery,
  local and owner-approved remote restart with completion observation.
- Versioned multi-capability model inventory, local/approved remote selections,
  owner role hints and explicit role-aware routing without hidden fallback.
- Node-origin job commands, bounded text output, durable creation correlation
  and receipt lookup after lost replies. No automatic mutation replay or
  exactly-once/deduplication claim.
- Remote settings/download admission recheck current owner approval; revoked
  approval cannot be restored by a stale remote write.
- Failed optional model/Python startup leaves deterministic node commands
  available. Fixed startup issue codes omit raw errors/paths. Model startup
  can still delay initial registration; full lifecycle control is pending.

See [commands](COMMANDS.md), [backend boundary](BACKEND-BOUNDARY.md),
[model startup limits](NODE_MODELS.md) and [detailed checkpoints](STATUS.md).

## Validation and security

All 154 Rust tests passed for the release source, including stable-only update
checks and install-time prerelease refusal. Earlier backend checks include
Rust tests with/without dashboard features,
isolated process tests using a fake inference backend, dropped-reply recovery,
permission revocation, optional-runtime failure, remote restart, first-run and
browser-copy regressions. Packaging adds runtime provenance, inventories,
Defender scan gates and isolated installer/upgrade tests. These do not prove
real inference, media operation or physical two-PC workload correctness.

Unsigned: no signing certificate is configured. Earlier second-PC Defender
reports remain unresolved; a local scan is not Microsoft clearance. Do not
disable protection, add exclusions or restore detected artifacts for testing.
Keep dashboards local. Existing owner permissions and authenticated transport
remain required. No private test address, credentials or special developer
access is embedded. No automatic analytics upload is added.
