# Read-only workload receipts

The existing command handles retained continuous-workload evidence as well as
ordinary attempts:

```text
eef requests receipt <job-id> --grant-id <grant-id>
eefn requests receipt <job-id> --grant-id <grant-id>
```

The second command uses the already running origin node, its existing submission
channel and the coordinator's saved request context. It cannot inspect another
origin's request. The first uses the existing local owner command endpoint. Both
keep the existing arguments and adapters; neither requires a dashboard. The IDs
come from retained job/issuance records. Public workload execution remains
incomplete, so this does not introduce a launch or restart command.

## Authentication and exact evidence

Workload inspection uses the existing framing and bounded pending-request map.
The current connection must use a separate node credential and pinned coordinator
TLS, and advertise `workload_receipts` with versions two and three. Missing,
disabled or incompatible advertisements fail before dispatch. There is no shared
key, ordinary-grant or generic capability fallback.

The read request carries the original exact scope, including its definition
digest. The node reads the existing journal, validates its version, owner, receipt,
definition and event prefix, and checks the full scope. The coordinator validates
the returned typed receipt and compares its full definition to the saved issuance.
Replies correlate to the current authenticated node and connection generation.
The historical session ID in a saved definition is evidence; reading it after
reconnect never restores that session or creates an issuer/lease.

Node reads use the existing bounded control and reply capacity. The actual storage
reader owns its permit if the network waiter disappears. No source factory,
permission mutation, model invocation, event append or journal repair is called.
Coordinator job loading follows the existing receipt service; broader bounded
asynchronous coordinator storage remains an outstanding dispatch requirement.

## Report compatibility and meaning

Ordinary receipts retain their schema-one report. Workloads use a strict schema-two
`coordinator_workload_receipt` envelope containing the original definition and a
typed node receipt. Accepted/unknown node evidence uses receipt schema two;
completed history uses schema three with its validated completion report. The
origin node retains its existing correlated wrapper and confirms the inner typed
report. Older clients cannot reinterpret it as an ordinary consumed receipt.

Reports show coordinator state as a revision snapshot alongside independently
retained node evidence. They do not change the coordinator revision or reconcile
an unknown job. Accepted does not establish that a source is still running.
Completion is retained node evidence, not physical-device attestation or permission
to replay. Missing/unavailable evidence does not establish that nothing happened.

Five new regressions cover exact read scope and unchanged journals, actual pinned
loopback TLS reads/reconnect/disabled support, legacy transport rejection, owner
and origin scoping with reopened coordinator history, and strict report/authority
validation through the existing command confirmation. Journal creation/consumption
and completion fixtures are synthetic; they do not prove capture or two-PC
acceptance. Fresh hosted compilation and native acceptance remain required.

A separate command integration case adds the real owner HTTP endpoint, real
coordinator submission handling, and two independently authenticated running
origin connections. Its executor is explicitly a synthetic receipt responder.
The owner and rightful origin must receive identical typed history; the other
origin must be refused before an executor read. The coordinator job must remain
issued, with its original revision, even when the node reports acceptance. This
checks command wiring and origin fencing without pretending to launch a source.
