# v0.4.0a8 candidate

This is a development prerelease candidate for matching coordinator and node
testing. It is not stable v0.4.0. Publication, installer acceptance and physical
two-PC acceptance are pending until recorded separately.

Compared with the published alpha.7, the candidate includes:

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

The two role installers remain separate. Installation includes runtimes, not a
model. Existing permissions, owner approval requirements and disabled stable-only
automatic update feeds remain in effect. Installing this prerelease is an
explicit choice; prerelease assets do not become automatic update candidates.

Source tests alone do not establish installer, antivirus, physical-node or stable
release acceptance. The [status record](STATUS.md) and
[two-PC test procedure](TWO_PC_TEST.md) distinguish those gates.
