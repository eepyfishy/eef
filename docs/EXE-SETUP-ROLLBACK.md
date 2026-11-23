# EXE setup rollback

Status: implemented in development and locally exercised for ordinary setup
errors. Not published, not stable-release acceptance, and not security-verified.
Automatic recovery after process termination or power loss is **not implemented**.
This does not change the separate [MSI rollback investigation](MSI-ROLLBACK-VALIDATION.md).

## Operation and recovery

Both wizard and quiet command-line setup use the same backend transaction.
While holding the existing `.update.lock`, setup covers:

- Application payload replacements and newly created files.
- Coordinator settings preparation, including a generated placeholder-secret
  replacement. Preparation works on a private staged copy first.
- The old `current.txt` selection and its retained manual-install backup.
- Enabling, replacing or removing the sign-in command entry.
- Creating or replacing the Start menu command entry.

Before publishing a replacement, setup stages and synchronizes the new bytes,
checks the observed original contents, and moves the old file to a unique
`.eef-setup-<original-name>-*.backup` beside it. Backups use renames on their own
filesystem, including shortcuts on another drive. Setup does not make another
full copy of the installed runtime. Existing owner settings and data remain
outside payload replacement, except for the installer's existing explicit
coordinator placeholder-secret preparation.

If a later step fails, changes are undone in reverse order before releasing
the setup/update lock. New files are removed; previous files and removed
startup entries are restored. The failed first installation can leave empty
directories and the reusable `.update.lock`; it does not recursively remove
directories or owner data. Retrying uses the ordinary installer again.

Recovery checks observed content hashes before restoring or removing files.
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

## Verification

Build the installer and its test-only `installer_launch_fixture` example, then:

```powershell
cargo test --locked --offline -p eef-installer-stub
.\tools\test-installer-rollback.ps1 -BinaryDir target/debug
.\tools\test-installer-launch-ui.ps1 -BinaryDir target/debug
```

The local increment passed 51 installer Rust tests. Failure coverage includes
partial writes after earlier publications, a real later payload path failure,
late Start menu failure after settings/selector/startup changes, fresh-install
retry, owner settings creation during staging, later edits, refused rollback,
unwinding, cleanup refusal and writes through an earlier original-file handle.
The Start menu tests verify that setup actually reached that step after the
earlier changes; an unrelated earlier error cannot satisfy the test.

The native rollback script runs six failure-and-retry cases: both roles, fresh
installation, and upgrades with startup on or off. It uses actual quiet installer
processes, deliberately blocks the isolated Start menu directory with a file,
compares every pre-existing file hash, checks that only the reusable lock remains
as an added file, then corrects the fixture and retries. Test programs are not
launched, and there is no production fault-injection option. The separate wizard
check covers 14 invalid-folder cases and four launch-recovery cases.

Local synthetic installer scans passed with Defender definitions `1.463.429.0`.
These scans do not resolve detections of published packages on another PC.
GitHub CI runs the quiet synthetic rollback script as an extra Windows/headless
step using its freshly built helpers. Its explicit `-SyntheticCi` mode is limited
to Windows GitHub runners, records `defender_scanned: false`, and makes no
Defender or full-package release claim. Normal local validation still requires
the existing Defender scan; no protection settings are changed.

## Remaining release gates

There is no durable transaction journal or automatic replay/recovery yet.
In particular, the old pathname can briefly be absent between its backup rename
and new-file publication. Process termination can leave a partially changed
installation and kept backups requiring recovery; the in-process checks do not
prove directory durability after power loss. A validated journal, bounded
recovery rules and interruption tests are required before claiming that gate.

Current full payload fresh/upgrade acceptance, physical two-PC operation,
installer signing/Defender reputation, production MSI rollback and stable v0.4.0
acceptance remain separate requirements. The synthetic fixture results do not
close them or authorize replaying interrupted application work.
