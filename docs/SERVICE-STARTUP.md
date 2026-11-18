# Service startup and concurrent offline commands

## Quiet startup investigation (development)

The separate capacity-hint candidate `c9ac8f4` passes its workspace tests, but
[run 44233145113](https://github.com/eepyfishy/eef/actions/runs/44233145113)
fails the native shell gate: the original quiet eef node exits with code 101.
The redirected diagnostic launch remains alive during its three-second sample
and does not explain that original exit. The run is failed, and its later native
vision checks were not reached. This is not evidence that the capacity feature
caused the exit, nor that an earlier startup issue is resolved.

The next diagnostic increment adds an explicit local launch option, currently
pending hosted verification:

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
one listener attempt. Hosted verification is pending. This source-level result
does not identify the cause of the earlier quiet process's code-101 exit.

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
