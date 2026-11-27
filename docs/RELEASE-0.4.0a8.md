# v0.4.0a8

This opt-in development prerelease improves setup recovery and command-service
responsiveness. Stable v0.4.0 acceptance remains open.

Compared with alpha.7, it includes:

- Setup recovery improvements and service startup refusal while a setup
  transaction needs recovery. See [setup recovery](EXE-SETUP-ROLLBACK.md).
- Bundled Python path fixes, including optional Windows imports from deeper
  installation directories. See [build verification](BUILD.md).
- Owner schedule Stop persistence on a separately owned native worker, with
  bounded observation and no automatic replay after a timeout. See
  [schedule commands](AUTOMATION-SCHEDULES.md).
- Bounded native journal reads for owner HTTP job inspection and authenticated
  origin job list/find/get/output. See [inspection scope](COORDINATOR-JOURNAL-READS.md).
- An optional build staging directory on another drive. Ownership checks prevent
  cleanup from adopting a pre-existing directory or traversing links. This is a
  build tool option, not a new execution isolation guarantee.

The two role installers remain separate. Language-model weights are not bundled;
each node owner selects or installs models. Existing permissions, owner approval
requirements and disabled stable-only
automatic update feeds remain in effect. Installing this prerelease is an
explicit choice; prerelease assets do not become automatic update candidates.

Clean build source is `f74943bc77b58410779501354f2ae4baad4498d8`.
The full local workspace run passes 1,951 tests with three ignored.
[All four CI jobs pass](https://github.com/eepyfishy/eef/actions/runs/45926840326),
including 1,980 Windows/headless Rust tests and all 40 native suites.

Both exact installers pass fresh installation, upgrades from alpha.6 with
settings preserved, complete immutable inventories, real packaged-process
interruption with startup refusal and recovery, and the full native wizard.
Local Defender definitions `1.463.488.0` pass both files. The builds are unsigned;
these scans do not establish Defender acceptance on other PCs.

The installed alpha.8 coordinator also passes accepted registration, six
authenticated pings and fresh-store job inspection across a coordinator restart
with a separate physical alpha.6 node. The remote installation and permissions
were unchanged. This is older-node compatibility; matching alpha.8 two-PC
workload and permission acceptance remains pending. See the
[two-PC procedure](TWO_PC_TEST.md) and [remaining gates](STATUS.md).

| Installer | Bytes | SHA-256 |
| --- | ---: | --- |
| `eef-installer.exe` | 23759784 | `8b7ae4b1f066f6ae7d56435a376b0b493acaade21ba1e554dd39b253eb25b3de` |
| `eefn-installer.exe` | 115153156 | `6d666206a244236f0e754299ae4024b70061e85c853984263937ec4d02067377` |
