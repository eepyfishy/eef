# Service startup and concurrent offline commands

## Standard-handle detachment correction (development)

The diagnostic runs for `14be628` and `601213f` expose a concrete corruption
problem: ordinary startup log bytes appear in the newly opened panic-report
file while the original quiet service is still live. Both fail the strict JSON
report gate. This explains those report-parse failures; it does not establish
the cause of the earlier `c9ac8f4` process exit with code 101.

Windows stores standard-handle values in a process table and validates them on
use; handle copies can be retained by code. See Microsoft's
[standard-handle lifetime guidance](https://learn.microsoft.com/en-us/windows/console/getstdhandle).
Rust documents null standard handles as the detached-console state, with standard
reads/writes completing without I/O. See
[Rust's Windows stderr behavior](https://doc.rust-lang.org/std/io/fn.stderr.html).
The old helper called `FreeConsole` while leaving the process's standard slots
unchanged, allowing later file handles to be reached through stale standard I/O.

The correction clears stdin, stdout and stderr slots before detaching only an
exclusively owned console with no redirected streams. It never manually closes
the old console handles. Checked service entrypoints propagate a failure instead
of starting after an unsuccessful transition. Failed slot updates do not detach;
if detachment fails, restoring the prior slots is best effort. Inherited terminals,
redirected pipes/files, command I/O and the legacy boolean helper remain compatible.

Three fault/order regressions exercise clearing before release, failed clearing
and failed detachment. A separate native helper uses an actual owned console,
requires null slots after detachment, then writes stdout/stderr while holding a
new unrelated file. Its sentinel must remain unchanged. The original real quiet
service/API/no-console gate, report parser and deadlines stay intact.

Exact source `50741ad` passes all four jobs in
[run 44301576706](https://github.com/eepyfishy/eef/actions/runs/44301576706),
including 1,863 Windows/headless Rust tests, one ignored across 18 groups, all 37
native suites and retained extra gates. Scoped source/raw-log review accepts
the actual console/sentinel case and both live quiet service checks, with empty
reports and preserved redirected command I/O. This also verifies the panic
observer and interruption correction below in the complete hosted suite. It is
not installed/physical-device or stable-release acceptance; the older code-101
cause remains open.

## Quiet startup investigation (development)

The separate capacity-hint candidate `c9ac8f4` passes its workspace tests, but
[run 44233145113](https://github.com/eepyfishy/eef/actions/runs/44233145113)
fails the native shell gate: the original quiet eef node exits with code 101.
The redirected diagnostic launch remains alive during its three-second sample
and does not explain that original exit. The run is failed, and its later native
vision checks were not reached. This is not evidence that the capacity feature
caused the exit, nor that an earlier startup issue is resolved.

The unreleased diagnostic increment adds an explicit local launch option,
verified at the `50741ad` checkpoint above:

```powershell
.\eefn.exe --panic-report .\node-panic.json --no-ui
.\eef.exe --panic-report .\coordinator-panic.json --no-ui
```

Use the same configuration and other service options as the intended launch.
Select a new file in an existing directory you own. The app refuses an existing
target instead of overwriting or appending to it; commands reject this service
option without creating a file. No file is created by default. The selected
package installs the observer after handoff, not an old bootstrap process.

The observer records at most one JSON document of 2 KiB from the first Rust panic
observed in that process. It includes the role, process ID, a best-effort startup
phase and source filename/line/column. It excludes full source/configuration
paths, panic payloads, backtraces, requests and credentials. Phase is the most
recent startup label, not proof of which task caused a fault. The already-open
handle is used once; panic handling never reopens or replaces the chosen path.

Normal panic handling and process behavior remain in effect. This does not catch
a panic, enable automatic recovery, replay work or change console handles. A
worker panic does not by itself establish that the whole service exited. An empty,
missing, malformed or unwritten report proves neither successful startup nor
absence of a fault; recording is best effort and cannot cover forced termination,
every native crash or failures before observer installation.

The native shell fixture now asks the original unredirected quiet process for
this observation. Its existing live API/process/no-console gate and deadline are
unchanged. A separate disposable example requires a real panic exit and verifies
redaction and refusal to overwrite owner data; it cannot satisfy quiet startup.
The cause of the observed code-101 exit remains open until actual evidence
identifies it. The earlier lock-contention fix below addresses a different proven
failure and does not explain this one.

## Console interruption availability (development)

A subsequent source review found a separate defect: both service entrypoints
discarded the `ctrl_c` listener's error result and treated any completion as an
exit request. The node's attention/restart wait had the same behavior.
The correction uses one shared interruption future that completes on a real
interrupt. If listener setup fails, it records a fixed warning and stays pending,
leaving normal management/restart controls available. It does not retry in a
ready-error loop. The node retains that future across nonterminal activation
notifications. Ordinary observed Ctrl+C still follows the existing graceful
cleanup path.

Two controlled-source regressions cover a successful signal result and a
failed listener with a later explicit management notification, including exactly
one listener attempt. The complete `50741ad` hosted checkpoint above also passes
the existing command restart/control and cleanup regressions. This result does
not identify the cause of the earlier quiet process's code-101 exit.

## Earlier lock-contention correction

This development follow-up fixes a reproduced silent-start failure. An offline
command and the service used the same configuration instance lock. If the
command won that lock just before launch, the service treated it as an existing
runtime and returned success without starting anything.

The failed Windows headless job in
[run 40243957515](https://github.com/eepyfishy/eef/actions/runs/40243957515)
records a node process exiting with code zero before any observed connection.
A controlled local test independently reproduces that outcome while a test-only
process holds the actual offline-command lock, with no existing service. The
hosted private process output is unavailable, so this proves the startup defect,
not that every detail of that hosted failure has been explained. The failed run
remains failed; it is not replaced by a replay or called accepted.

## Ownership and behavior

Both eef and eefn service entrypoints now use two lifetime guards:

- The existing configuration `.instance.lock` still excludes offline commands
  while a service owns configuration. Existing commands and legacy interfaces
  continue to use it unchanged.
- A separate `.service.instance.lock` distinguishes a current service launch
  from an offline command. Only service startup acquires this new guard.

A new launch first acquires the service guard, then waits cooperatively for up to
five seconds for the existing command lock. It does not initialize settings,
start an API/model/gateway, or replay the command while waiting. A second current
service launch sees the service guard and reuses the starting/running service;
it cannot take over its initialization. This is ownership, not a certificate of
HTTP readiness, healthy inference or completed registration.

Persistent command contention returns a startup error, never already-running
success. An older running binary that owns only the legacy lock is also treated
conservatively: it is neither evicted nor bypassed. Let a command finish or stop
the existing service normally before explicitly launching again. Filesystem
errors remain errors. The cooperative wait does not preempt blocking filesystem
calls or promise hard real-time startup deadlines.

Cancellation, failed acquisition and normal shutdown release acquired guards.
Both roles retain the existing explicit guard drop before update handoff. Stale
unheld lock files do not establish ownership; do not delete lock files to bypass
a live process. No service manager, elevated helper, automatic restart loop,
permission/model change, firewall rule or mutation replay is introduced.

## Verification

Seven focused Rust tests cover held/released guards, command contention, pending
duplicate startup, cancellation, timeout, independent configurations and
filesystem errors. The native `tools/test-service-startup-lock.mjs` fixture uses
an independently held real command lock and actual service executables. It
requires observable waiting, no early configuration/API initialization, one
starting owner, successful startup after release, matching ordinary command
runtime, persistent-contention failure and an explicit later launch. Its tiny
`instance-lock-fixture` helper is test-only and never an installed payload.

CI retains the original node-output suites and twenty-second deadlines, adds
the startup fixture, and still requires genuine first initialization. Full
workspace, native, browser and exact-revision hosted results require their own
acceptance. This is a startup reliability increment, not completion of model
onboarding, physical two-PC, MSI recovery, signing/Defender or stable v0.4.0.
