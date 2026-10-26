# Service startup and concurrent offline commands

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
