# v0.4.0a9

This opt-in Windows prerelease adds shared installation ownership for running
services. Setup refuses to change a cooperating installation while any of its
service profiles are still running, including direct launches from a versioned
directory. Setup can proceed after the last service exits. Stop the apps and
back up their settings and data before upgrading.

The coordinator and eef node remain separate installers. Model weights are not
bundled or downloaded by setup; node owners choose their models and permissions.
Automatic-update feeds remain paused and exclude prereleases.

Clean package source is
`c179ce273b31acc9b1e372885cda6f325019a97f`, internal version
`0.4.0-alpha.9`. All four
[source verification jobs](https://github.com/eepyfishy/eef/actions/runs/46107812265)
pass: Windows/headless has 1,995 passing Rust tests, three ignored and all 41
native suites; Windows/dashboard has 1,966 passing tests and both Linux jobs have
1,894. The exact-source
[Windows application build](https://github.com/eepyfishy/eef/actions/runs/46107812285)
also passes. Packaging verifies the hosted artifact and assembles the normal
Python/runtime bundles without compiling Rust locally.

Both exact installers pass isolated fresh installs, real alpha.8 upgrades with
configuration preserved, complete immutable inventories, six production
installation-lifetime cases using real versioned services, actual installer
interruption with startup refusal and recovery, and full native wizard
installation. Runtime import probes pass for both roles. Retained reports,
inventories, validator copies and hashes were independently checked after the
temporary test payloads were removed.

Local Microsoft Defender scans pass with definitions `1.463.507.0`. Both files
are unsigned. These results do not establish signing, a Microsoft false-positive
verdict or Defender acceptance on another PC. Keep detected files quarantined.

The guard depends on cooperating new builds; older services/installers and
external processes do not acquire it. It is not authorization or process
isolation. Matching two-PC workloads, real models/media, resource budgets,
versioned firewall/update lifecycle, disk-full/power-loss and production MSI
acceptance remain open. Stable v0.4.0 is not accepted. The newer task-history
follow-up on the development branch is not included in this frozen source.

| Installer | Bytes | SHA-256 |
| --- | ---: | --- |
| `eef-installer.exe` | 23767957 | `1517190affd922d262dbf01873e53f93ccc6d408de30556f553535f6491353f3` |
| `eefn-installer.exe` | 115160442 | `afd8d8b99c30a87cf243fb22c99dd7c35340a2d14d80eb28d609ff586bd91fd2` |
