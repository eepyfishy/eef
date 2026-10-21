# Atomic settings and backup publication (unreleased after alpha.6)

Node first-run JSON, normal node settings, coordinator YAML and the installer's
initial coordinator-secret save now share one same-directory staging/replacement
helper. The former rename-error fallback
copied over the live destination; it is removed. Existing fixed `.next` siblings
are not reused, truncated, restored or deleted by a new write.

## Contract

Serialization/validation happens before writing settings. Each write creates a
unique temporary file beside its destination, writes all bytes, synchronizes that
file, and uses the existing `tempfile` dependency's
[atomic persist operation](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html#method.persist)
to publish it. No new package/version is downloaded: the installer promotes its
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
ignored profiler each**: EEF 271, owner CLI 21, installer 14, EEFN 626 and node CLI
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
