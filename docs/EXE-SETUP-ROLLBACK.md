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

## Service startup during pending setup (development)

Current development applications inspect setup state before selected-package
handoff, console detachment, identity preparation or service startup. An
uncommitted journal blocks a new service and asks for the matching installer.
Corrupt, unknown, wrongly bound or out-of-scope records also block startup;
they are not interpreted as successful setup. The installer and applications
share the existing bounded version-1 decoder rather than separate commit-marker
heuristics. A commit requires all prepared changes to have been applied and
none to have been undone.

A validated committed journal permits startup with a cleanup warning. This
preserves the existing successful-install behavior when backup or journal
cleanup fails. A partial final cleanup append does not undo a validated commit.
The application does not recover, truncate, delete backups or acquire the
installer's update lock. Any remaining journal still blocks another package
mutation until the matching installer handles recovery. Help, version and
offline package inspection remain available.

This is a point-in-time check by matching development executables, not a lock
held for a service's lifetime. It does not stop an already running application,
retrofit older binaries, authenticate checksummed records, establish isolation
against local processes or guarantee startup across concurrent path changes.
It is not included in the published alpha.7 packages or the `afc2f2f` packages
described below. Complete package, older-tool and physical acceptance remain
separate from the executable-level checks.

`tools/test-setup-startup.mjs` exercises both real development executables with
private configuration and synthetic managed-package metadata. It checks startup
refusal before settings, database or listener activity for interrupted and
invalid journals, refusal before selected-package handoff, committed cleanup
startup with evidence untouched, and available help/version/offline inspection.
It supplies no model, runtime probe, real node or production package.
Read-only compatibility is separately checked against four journals captured
from the previous installer writer: both roles, before and after commit. That
explicit-input test is ignored in ordinary CI because its historical local
fixture trees are not shipped with the repository.

For a matching full-package candidate with this guard, the ordinary packaged
interruption driver accepts `-VerifyStartupAdmission`. After verifying that the
candidate executable was published and the real journal is still uncommitted,
it runs that executable in place before the ordinary recovery retry. Private
invalid settings provide a negative control: a configuration or loader error
cannot count as refusing unfinished setup. The check requires the exact setup
error, exit code 1, unchanged journal/settings and no database creation. It
starts no node/coordinator service, model or network work. The switch is optional
so the driver can still exercise earlier packages that lack startup admission.
Its helper passed local positive/negative checks for both roles with generated
journals and current/preceding application binaries; actual full-package evidence
must be recorded separately.

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

## Full-payload development probe (2026-11-26)

The journal checkpoint `1109085` passed
[all four hosted jobs](https://github.com/eepyfishy/eef/actions/runs/45320784886).
Its Windows/headless log contains 1,962 passing Rust tests plus one ignored,
the exact 39 native command suites, the retained extra case and the separate
quiet installer fixture. Logs were reconciled with that commit's workflow.

A local optimized, static-CRT release-profile installer was then built from
clean `11090856bb432137b0ce937b700d87abbeb3b19f` using Rust 1.98.1. Its SHA-256 is
`5067a26d2b5318174984bc61962402a8b9ec104f6c7bec134983bf6c353d889d`.
Import inspection found no LLVM-MinGW runtime DLL dependency. This tests a
standalone installer build rather than a debug stub requiring compiler DLLs.

The probe retained the exact published alpha.7 payload bytes from
`af4dcf495bd53196e3f423ccd0c9b1c341bda754` and replaced only the installer stub.
It is deliberately a **mixed-source fixture**, not a new release or a package
claiming that its application executables come from the journal checkpoint.

| Fixture | SHA-256 |
| --- | --- |
| eef installer | `cfff2ca6adb0ee2d7f8a9f99b7b1c5fcdfea93c9228628b0d8e5cd905ef4d042` |
| eef node installer | `024bc10b53fc6e687ec9365652293eac7a4e943a30a0d2c6a62e2db63b41e113` |

Fresh installation and actual upgrades from the published alpha.6 installers
passed for both roles with configuration and both startup choices preserved.
The eef inventory has 54 records; the node inventory has 2,287. A separate audit
of both fresh and upgraded trees verified all 53 and 2,286 immutable file hashes,
respectively; the one mutable configuration record per role is checked by the
separate preservation checks. Bundled media imports and the llama.cpp version
probe passed in both node trees. Original installation reports were kept intact.
The checker is now shared with the installer test so future upgrade runs also
verify the entire immutable inventory and repeat the runtime probes.

Defender scans of these fixture installers and the previous installers passed
with definitions `1.463.471.0`. Larger fixtures used an explicit scratch base
in the system temporary directory, leaving E:'s limited space available. No
model was downloaded or selected, no node/coordinator service was launched,
and no release or remote node update was performed.

The normal packaged binaries also passed forced-termination recovery for both
roles during real alpha.6 upgrades. `tools/test-installer-interruption.ps1`
started each installer in an isolated directory, observed the replacement
executable, and killed only the process handle it had created. After termination,
the expected new executable hash was present and the checksummed journal was
uncommitted. The eef case retained 27 prepared and applied changes; the node case
also retained 27, alongside 2,279 staging records. An ordinary installer rerun
recovered and completed, preserved configuration and an unrelated owner file,
and left no setup journal, stage files or backups. All 53/2,286 immutable files
passed again, as did the node's bundled runtime probes. These are the same
mixed-source fixtures described above, without a production interruption hook.

An earlier observation attempt missed the interruption window while hashing
the executable and parsing the live journal; setup had removed the journal
before inspection after termination. That attempt remains a failed observation.
The final driver keeps live observation short, then verifies the stopped
process's executable bytes and full journal before attempting recovery. Missing
or committed journals and unexpected executable hashes still fail the check.
Successful and failed reports were retained separately with installer digests;
the final successful reports also bind the driver's digest.

## Matching-source long-path regression (2026-11-26)

Clean `47eabcd8c3a078418fd4427a66ab4172ff2d60da` passed 1,933 local
workspace/all-target tests with one ignored and
[all four hosted jobs](https://github.com/eepyfishy/eef/actions/runs/45777532433).
The standard bundler built matching application payloads and a static installer,
passed standalone startup/media imports and Defender scans, and completed with
an observed native exit of zero. The earlier runner observation that lost its
child exit code and its original artifacts remain separately retained.

Full-package installation then found an actual node failure. A NumPy license's
final path was 248 UTF-16 units, but the longer temporary filename exceeded the
legacy Windows path limit. The staging helper's native Windows calls received
an ordinary path and returned error 3. The node setup rolled back; independent
inspection found only the reusable lock and empty directories in that fresh
destination. The coordinator's fresh installation and alpha.6 upgrade passed,
but the combined installer suite failed and is not accepted.

Setup now resolves staging and identity-probe parents before creating temporary
files, preserving Windows' canonical extended prefix through publication.
It does not change a registry setting, shorten the destination, change the
journal schema or relax ownership checks. Two Windows regressions reproduced
the failure before this change and now pass: a short final filename whose
staging path exceeds the limit, including rollback and commit, and uncommitted
recovery with the installation root itself beyond the limit. All 68 installer
tests pass, including the existing 16 owned-process termination cases. Full
matching-package reruns are recorded below.

### Matching-package acceptance for `afc2f2f`

Clean `afc2f2f6d3d048a7cd5d80da727a9986ac66859e` passed 1,935 local
workspace/all-target tests with one ignored and
[all four hosted jobs](https://github.com/eepyfishy/eef/actions/runs/45783765642).
Raw-log reconciliation confirms 1,964 passing Windows/headless Rust tests with
one ignored, all 39 native suites, the extra registration-close case, the quiet
installer fixture and the nine Cargo output-directory checks.

The standard bundler produced matching application and installer sources with
an observed native exit of zero. Both exact installers passed Defender with
definitions `1.463.471.0`. Their static installer prefix matches the compiled
stub's digest and has no LLVM-MinGW runtime DLL import or interruption hook.

| Development installer | Bytes | SHA-256 |
| --- | ---: | --- |
| eef | 23,711,288 | `0546ce845cf3498c9956ef4673946cc69b9db777e88e596e2ffee6d97981bc03` |
| eef node | 115,112,423 | `a18648c1160cff1d08187191fda00cd3eb2f870787db5e1e5133330716d1acc4` |

The node installer succeeded at the original failing destination, including the
same 248-unit license path, without shortening the folder or changing Windows
policy. The complete fresh-install and actual alpha.6-upgrade suite then passed
for both roles. It preserved configuration, both startup choices, retained
versions and backed-up update selection. All 53/2,286 immutable files verified
after fresh installation and upgrade; node media imports and llama.cpp startup
also passed. The native wizard completed installation for both roles, including
review-before-write and Back preserving choices.

Both normal packaged installers passed an owned-process termination during
upgrade: expected new executable bytes were present while the journal remained
uncommitted. An ordinary rerun recovered and completed, preserved settings and
unrelated owner data, verified the complete inventory and left no setup scratch
or journal. No production halt hook was used. The installed binaries also passed
[private local enrollment](LOCAL-ENROLLMENT.md) with two logical nodes, explicit
restart consent and a lost reply that did not replay enrollment.

These remain local development packages at internal version `0.4.0-alpha.7`.
No release asset, update feed or physical node was changed. Local process tests,
runtime imports and scans do not establish physical two-PC, real-model/media,
power-loss, disk-full, signing or stable v0.4.0 acceptance.

## Remaining release gates

An old pathname can briefly be absent between its backup rename and publication;
recovery requires running the matching installer again. This is not continuous
application availability. Untested filesystems/network shares, actual power-loss
and disk-full behavior, orphan metadata from interruption before its first
journal record, and startup/older-tool interaction beyond the development
point-in-time check require further acceptance.
The process tests do not prove directory durability after power loss.

Physical two-PC operation, installer signing/Defender reputation, production
MSI rollback and stable v0.4.0 acceptance remain separate requirements. Successful
recovery does not authorize replaying interrupted application work. The
development startup guard distinguishes unfinished setup from committed cleanup
warnings; it is not a service-lifetime or older-binary guarantee. The separate
package-mutation guard deliberately blocks both states.
