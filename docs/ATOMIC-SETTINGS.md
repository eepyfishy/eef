# Atomic settings and backup publication (unreleased after alpha.6)

Node first-run JSON, normal node settings, coordinator YAML and the installer's
initial coordinator-secret save now share one same-directory staging/replacement
helper. The former rename-error fallback
copied over the live destination; it is removed. Existing fixed `.next` siblings
are not reused, truncated, restored or deleted by a new write.

## Windows live-reader follow-up (in development)

The `fcd559f` hosted run fails in the repair-receipt fixture: its readiness poll
combines a CLI snapshot captured at `admitted` with a later TCP connection, then
incorrectly assumes that older snapshot already says `downloading`. The poll now
requires the exact operation's active, in-progress downloading receipt in the
status itself. Seven deterministic snapshot cases cover admitted, terminal,
wrong-ID, duplicate and unavailable evidence; the original state assertions and
thirty-second observation bound remain. A helper confirmed exited before that
phase causes an immediate test failure, not prolonged polling or action replay.

Four fresh local fixtures pass that correction, but a fifth retains an independent
`Access is denied` admission failure with an empty journal and an unadmitted lease.
No operation is resumed or reset. An isolated Windows reproducer then fails at
the existing `tempfile`/`MoveFileExW` replacement while ordinary readers are active.
This is separate from the earlier snapshot race and from antivirus clearance.

Windows replacement now uses one
[`SetFileInformationByHandle`](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle)
rename through a reopened staging handle, with replace/POSIX semantics, after
the existing complete write and synchronization. Ordinary DELETE access to an
existing target is required; read-only-ignore, ACL-bypass, copy fallback, retry,
sleep and recovery flags are not used. Old reader handles retain their complete
snapshot while new opens see the published file. Delete-denying handles still
refuse replacement. No-clobber creation and non-Windows publication are unchanged.
Unsupported native information classes/filesystems fail closed; this is not a
new certification of network shares or older Windows releases.

The isolated prototype records 5,000 successful publications with four concurrent
readers, 265,306 reads and no failed reads or invalid record prefixes. Fixed-length
shared-source tests separately check complete payloads. A negative native
experiment with a delete-denying handle refuses publication without changing the
original. Shared Rust tests cover open snapshots, fixed-length concurrent reads,
Unicode names, read-only files and the previous delete-lock/directory/partial-write
failures. Both full local Rust modes pass 1,138 tests plus one ignored profiler
each. Fresh applications pass all thirty hosted-selection native suites, two
cached-runtime native suites, five additional distinct receipt fixtures and the
unchanged first-run browser regression against frozen source and eleven native
artifact hashes. A separate executable including the actual shared source
completes 5,000 writes and 4,033 fixed-length validated reads without publication,
read or incomplete-record errors. Its initial loader failure (`0xc0000135`)
precedes any journal creation and remains retained; a process-local search path
to the already-cached compiler runtime allows the unchanged executable to run.
Neither completed native suite is replayed and no DLL is downloaded or installed.

The separate cached real-CPU owner workflow passes private enrollment, reviewed
work, identity-preserving restart, retained results/no replay, model withdrawal,
explicit reload and automatic idle unload without a node restart. No model is
downloaded or copied; this verifies operation mechanics, not interpretation quality
or a default model. The independent read-only reconciliation is retained in
`.validation/atomic-reader-final-acceptance.json`; all fixture processes are
retired and evaluated source/artifact hashes match. New hosted follow-up remains
separate and pending, not accepted by these local checks.
No new dependency version, model, installed user application or update policy is
changed by this source increment. Custom ACL preservation and power-loss/multi-file
transaction guarantees are still not claimed.

## Contract

Serialization/validation happens before writing settings. Each write creates a
unique temporary file beside its destination, writes all bytes, synchronizes that
file. Non-Windows replacement and no-clobber creation use the existing `tempfile` dependency's
[atomic persist operation](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html#method.persist)
to publish it; Windows replacement uses the native path described above. No new package/version is downloaded: the installer promotes its
already-locked tempfile test dependency to a normal dependency and includes the
small shared module, not the node runtime. The
[standard rename semantics](https://doc.rust-lang.org/std/fs/fn.rename.html)
are replacement semantics, not permission to use a non-atomic copy after failure.

Before publication, a write/sync error or refused replacement leaves this writer
without a path to partially overwrite the live destination. Temporary-file
cleanup owns only its newly created file; it never sweeps siblings or user data.
Normal errors clean up that staging file. Forced process death, external locks
or filesystem failures can leave an orphan; no automatic orphan repair/deletion
is claimed. Windows locks that deny deletion/replacement cause a save failure,
even when the handle permits ordinary writes. There is no lock or antivirus
bypass and no client fallback/replay.

The existing node `.json.bak` and coordinator `.yaml.bak` paths are unchanged.
Backups use the same staging/sync/replacement path, streamed from the current
settings file. A missing source or failed backup replacement cannot partially
overwrite the last backup. If backup publication fails, settings publication
does not begin. A later failure publishing settings may leave the backup updated
to the still-current settings; these are **two separate file replacements**, not
one multi-file transaction.

The node sets its pending-restart hint and wakes its setup supervisor only after
the relevant settings save succeeds, as before. There is no application restart,
policy reset, model selection, permission change or reinterpretation hidden in
the storage helper. First-run identity/enrollment and local API discovery JSON
also inherit the helper through `setup::write_json`; their validation, locks and
best-effort discovery error handling stay unchanged.

The installer still preserves a valid existing coordinator secret byte-for-byte.
If it needs to generate an initial secret, failure publishing the replacement
leaves the existing configuration intact. Extraction, shortcuts, registry/startup
choices and application-version selection retain their separate contracts; no
new installer payload is produced or tested by the configuration helper itself.

The helper does not add a cross-process settings lock or serialize independent
writers to the same destination; existing owner/configuration locks remain
responsible for read/modify/save semantics. Unique staging files prevent distinct
destinations from sharing a temporary filename. Unix temporary files retain the
dependency's owner-private creation permissions; Windows inherits the configured
directory's access controls. Custom ACL/metadata preservation is not promised.

File synchronization before publication is not a complete power-loss durability
guarantee: the containing directory is not separately synchronized, filesystems
may differ, and no power-cut or real disk-exhaustion certification is claimed.
SQLite journals, application updates, model artifacts and arbitrary user file
operations keep their own existing persistence contracts.

## Verification

Six regressions fail against the original writers: both first-run and normal node
saves and coordinator saves consume an existing `.next` sibling and report success
after rename is denied by a native Windows delete lock. The pre-fix observations
are retained in .validation/atomic-settings-before.log and
.validation/atomic-settings-before-all.log. The installer secret-creation
regression also fails against its original writer, retained in
.validation/atomic-settings-installer-before.log. Its existing-secret no-op
already passed and remains preserved.

All twenty focused test executions pass in
.validation/atomic-settings-complete-after.log. These are fourteen distinct new
regression functions, with six shared-helper cases exercised in both node and
installer builds. They cover original writer failures, backup locks, partial
write failure, concurrent different destinations, absent copy sources, refusal to
replace a directory, cleanup and installer secret preservation/recovery.

Both locked/offline workspace/all-target feature modes pass **959 tests plus one
ignored profiler each**: eef 271, owner CLI 21, installer 14, eefn 626 and node CLI
27. Final logs and build:

- .validation/atomic-settings-workspace-default-accepted.log
- .validation/atomic-settings-workspace-headless-accepted.log
- .validation/atomic-settings-build-accepted.log

Final headless apps/examples build. Public settings-storage-uvPNhG uses the actual
owner APIs and a native test-only read handle withholding FILE_SHARE_DELETE.
Node and coordinator primary and backup replacement are refused without
overwriting the protected file; failed node saves do not set pending restart;
legacy `.next` siblings survive; normal saves recover after lock release without
restarting the writer; staging files are cleaned. Saved preferences still need
their normal explicit application. The earlier settings-storage-Ys6XP2 probe is
retained as an intermediate checkpoint, not the final build's evidence.

Fourteen existing native suites also pass against the final evaluated build:

- model-setup-Zjg9od, model-activation-jnSn6d, model-bootstrap-bnnQPJ
- unavailable-model-QUFJY0, model-metadata-Pnw9Tw, installed-selection-XwOWLY
- process-lifetime-vqt0ou, local-pipelines-hiv8td, python-policy-esj38f
- python-bridge-lOg6SS, permission-commands-28KhwM, node-commands-cZ1A42
- node-credentials-5zNHGT, model-downloads-ey7sPs

Each fixture retains results.json (Python bridge uses result.json). Combined
native output is .validation/atomic-settings-native-accepted.log. Frozen source
hashes are in .validation/atomic-settings-accepted-source-hashes.json; five
evaluated app/helper hashes are in .validation/atomic-settings-final-native-hashes.json.
All match after acceptance. Dependency-boundary checks also scan shared Rust
modules; formatting, JS syntax, ignored staging names and diff checks pass.
Fixture apps/helpers and owned model servers are terminal.

Tests use owned temporary configurations and injected partial I/O failure, not
user files, model downloads, physical two-PC acceptance, actual disk exhaustion
or power cuts. Installer secret creation is covered by native Windows Rust unit
tests, not a new packaged-installer end-to-end certification.

Full first-run dependencies, approved model quality, physical two-PC and
installer/signing/Defender/stable-release acceptance remain open. No installed
or remote node, model default, visual UI, version, release installer package or
update policy is changed by this source increment.
