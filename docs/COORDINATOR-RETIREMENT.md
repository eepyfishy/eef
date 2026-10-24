# Coordinator restart and journal ownership

Unreleased after alpha.6. An in-process EEF restart must retire its previous
job-database owner before initializing a replacement runtime. Shutdown intent,
an empty runner set and a dropped root `Runtime` reference are not sufficient:
other final callbacks or service references can still own the journal.

The native model-bootstrap run `model-bootstrap-bfvMBT` observed a coordinator
exit on restart with `Another coordinator already owns this job database`.
The precise late holder was not identified after that fixture had exited. Its
private logs establish the lock refusal, not which callback held the final
reference. The failed fixture is retained and is not replayed or accepted.

## Lifecycle boundary

EEF still closes admission and drains runners, background work, node transport
and HTTP through the existing shutdown path. After HTTP draining, main consumes
the runtime through `Runtime::retire`. It takes a receiver tied to that exact
`JobStore`, drops its root runtime reference, and awaits the store's retirement
signal before attempting the next initialization once.

The signal owns no runtime, journal or lease. Its guard is deliberately the last
`JobStore` field: the SQLite connection and exclusive lock file must be dropped
before the signal announces retirement. Preserve this field-drop ordering.
Independent journal clones therefore delay replacement even after the root
runtime itself has gone away. The existing OS exclusive-lock check is unchanged.

Main allows five seconds for this final ownership retirement. This is an
event-driven wait, not a fixed startup delay or retry of `JobStore::open`.
Timeout or lost evidence stops the restart rather than unlocking another owner,
deleting a lock file or starting a second runtime. Another process acquiring the
database still causes the ordinary exclusive-lock refusal.

`runtime.stopped` remains a shutdown event, not proof of final journal retirement
or physical completion of arbitrary external effects. The restart command still
acknowledges a request; callers must verify a new runtime and node reconnection.
No job is automatically replayed by this boundary. Existing interruption and
recovery semantics remain responsible for durable work.

A normal shutdown or selected-version handoff uses this same retirement
boundary; this is not a certificate for abnormal exits or failed initialization.
It does not authorize an update, choose a version, restart a node,
change permissions, relax prerelease exclusions or grant cluster leadership.

## Evidence scope

Six focused `retirement` tests pass, including three new tests for final journal
ownership, timeout refusal and runtime drop with independently retained stores.
Rebuilt EEF passes `model-bootstrap-l6uYv7`: ten distinct runtime IDs across
nine explicit restart requests, plus refusal of a competing coordinator opening
the active database. Those are fresh operations, not automatic recovery retries
of the failed request. Existing restart-observation deadlines are unchanged.

The final combined maintenance-policy, activation and retirement source passes
both locked/offline workspace/all-target feature modes (1,089 tests plus one
existing ignored profiler each), a fresh headless app/helper build and all
twenty-nine native suites in one sequential run. `model-bootstrap-WH2CyR`
repeats the ten-runtime/competitor checks in that run. All 299 source and nine
native artifact hashes match in `.validation/package-policy-acceptance.json`;
no matching fixture processes remain observed. Earlier failed and focused
results remain distinct from this full acceptance. Hosted CI is pending.

This does not establish physical two-PC, package migration/repair,
signing/Defender, model quality, quorum failover or stable v0.4.0 release acceptance.
