# EXE setup rollback

Status: implemented in development and locally exercised for ordinary errors
and forced process termination. Not published, not stable-release acceptance,
and not security-verified. Directory durability after power loss remains unproven.
This does not change the separate [MSI rollback investigation](MSI-ROLLBACK-VALIDATION.md).

## Operation and recovery

Both wizard and quiet command-line setup use the same backend transaction.
While holding the existing `.update.lock`, setup covers:

- Application payload replacements and newly created files.
- Coordinator settings preparation, including a generated placeholder-secret
  replacement. Preparation is a pure byte transformation before publication.
- The old `current.txt` selection and its retained manual-install backup.
- Enabling, replacing or removing the sign-in command entry.
- Creating or replacing the Start menu command entry.

Before publishing a replacement, setup stages and synchronizes the new bytes,
checks the observed original contents, and moves the old file to a unique
`.eef-setup-<name-digest>-*.backup` beside it. Short derived names also support
original filenames at the filesystem's component-length limit. Backups use renames on their own
filesystem, including shortcuts on another drive. Setup does not make another
full copy of the installed runtime. Existing owner settings and data remain
outside payload replacement, except for the installer's existing explicit
coordinator placeholder-secret preparation.

If a later step fails, changes are undone in reverse order before releasing
the setup/update lock. New files are removed; previous files and removed
startup entries are restored. The failed first installation can leave empty
directories and the reusable `.update.lock`; it does not recursively remove
directories or owner data. Retrying uses the ordinary installer again.

Recovery checks observed content hashes and filesystem file identities before
restoring or removing files. Identical bytes in a different owner file do not
authorize replacing that file.
An observed owner edit, changed backup, refused rename, or unavailable file
causes an explicit recovery error. Setup continues recovering independent
changes and keeps the failed change's backup, including its path in the error.
These checks are conflict detection, not an isolation boundary against
untrusted local processes or a guarantee against every concurrent path race.

After successful setup, backup cleanup errors become warnings rather than a
false installation failure or a new rollback attempt. The wizard shows them on
its completion page; quiet setup writes them to stderr. A backup changed through
an earlier open app handle is retained and reported. Close running apps before
installing over their files.

Launch follows commit. A launch failure does not roll back an application that
may already have started work or written owner data. The wizard's existing
**Retry opening** and **Close** choices remain separate from installation.

## Interrupted setup

Run the matching installer again and choose the same installation folder. In
the wizard, recovery starts after **Install**, not while inspecting Options or
Review. The quiet command uses the same backend. Recovery runs under the
setup/update lock before checking space for another full payload, so it can
release tracked files left by the interrupted attempt first.

The version-1 `.setup-transaction` journal records staging identities before
writing temporary contents and records planned changes before moving old files.
It uses bounded, checksummed append frames: 256 KiB per record, 64 MiB total,
and at most 100,000 staged files or changes. This avoids repeatedly rewriting
the full file list for a large Python bundle. A filesystem identity/rename probe
runs before using a volume; unsupported identity behavior fails without a
fallback that guesses ownership from filenames or content alone.

Uncommitted setup is undone in reverse order. A committed setup keeps the new
installation and only finishes backup cleanup. Recovery progress is recorded
so another interruption can be handled again. A partial final append is ignored
only after the complete prefix, root, role, current shortcut destinations and
all affected paths validate. Corrupt complete records, unknown formats,
redirected parents, outside paths and changed file identities are refused before
restore. Checksums do not authenticate an owner or establish a security boundary.

Matching development clients refuse updates, update rollback, package repair
and repair-proposal retention while this journal exists. Read-only diagnostics
remain available. Older binaries do not gain that guard retroactively. Keep the
journal and reported backups when recovery needs attention; do not delete them
to make another package operation proceed.

Archive paths are normalized before checking settings preservation and reserved
setup filenames. Dot/separator/case aliases cannot turn owner configuration into
a payload replacement. Windows device names, alternate-stream names and unsafe
components are rejected before creating the installation destination.

## Verification

Build the installer and its test-only `installer_launch_fixture` example, then:

```powershell
cargo test --locked --offline -p eef-installer-stub
.\tools\test-installer-rollback.ps1 -BinaryDir target/debug
.\tools\test-installer-launch-ui.ps1 -BinaryDir target/debug
```

The local increment passed 66 installer Rust tests. Failure coverage includes
partial writes after earlier publications, a real later payload path failure,
late Start menu failure after settings/selector/startup changes, fresh-install
retry, owner settings creation during staging, later edits, refused rollback,
unwinding, cleanup refusal and writes through an earlier original-file handle.
The Start menu tests verify that setup actually reached that step after the
earlier changes; an unrelated earlier error cannot satisfy the test.

Sixteen forced-termination cases cover both roles at staging creation, completed
staging write, preparation, old-file move, publication, commit, partial backup
cleanup and rollback. The harness kills an owned child process using the actual
setup backend, then checks original restoration or committed-state preservation
and a successful new setup. Normal retry integration also verifies recovery
before further publication and lock exclusion during that transition.
The halt checkpoints exist only under `cfg(test)` in the Rust test executable.
Normal installer binaries are separately checked for the fixture environment
names and marker; there is no production interruption hook.

Another 51 focused node tests cover the updater, repair, retained sources,
shared setup guard and coordinator configuration. Corruption/scope tests check
that later bad records cannot cause earlier undo, preflight remains read-only,
same-content replacement files are retained, and frame/entry limits fail closed.

The native rollback script runs six failure-and-retry cases: both roles, fresh
installation, and upgrades with startup on or off. It uses actual quiet installer
processes, deliberately blocks the isolated Start menu directory with a file,
compares every pre-existing file hash, checks that only the reusable lock remains
as an added file, then corrects the fixture and retries. The local check used
installation paths on E: and isolated AppData paths on C:, recording both roots.
Test programs are not
launched, and there is no production fault-injection option. The separate wizard
check covers 14 invalid-folder cases and four launch-recovery cases.

Local synthetic installer scans passed with Defender definitions `1.463.440.0`.
These scans do not resolve detections of published packages on another PC.
GitHub CI runs the quiet synthetic rollback script as an extra Windows/headless
step using its freshly built helpers. Its explicit `-SyntheticCi` mode is limited
to Windows GitHub runners, records `defender_scanned: false`, and makes no
Defender or full-package release claim. Normal local validation still requires
the existing Defender scan; no protection settings are changed.

## Remaining release gates

An old pathname can briefly be absent between its backup rename and publication;
recovery requires running the matching installer again. This is not continuous
application availability. Untested filesystems/network shares, actual power-loss
and disk-full behavior, orphan metadata from interruption before its first
journal record, and startup/older-tool interaction require further acceptance.
The process tests do not prove directory durability after power loss.

Current full payload fresh/upgrade acceptance, physical two-PC operation,
installer signing/Defender reputation, production MSI rollback and stable v0.4.0
acceptance remain separate requirements. The synthetic fixture results do not
close them or authorize replaying interrupted application work.
