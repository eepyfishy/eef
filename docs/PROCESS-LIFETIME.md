# Bounded process lifetime (unreleased after alpha.6)

`process.exec` now owns its output readers in the executing future, rather than
detached Tokio tasks. Its existing timeout covers both the root process and pipe
collection. Permission switches/allowlists, exact grant scope, local policy
checks, result limits and unknown-effect reporting remain unchanged. No dashboard
controller, shell command construction, administrator bypass or new public
execution path is introduced.

## Windows containment

Each bounded command gets an unnamed, non-inheritable Windows Job Object with
`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The root starts suspended with the
no-console-window creation flag (not a promise to hide GUI applications),
joins that job, then resumes its verified primary thread. Failure to configure,
assign or verify/resume terminates the suspended/contained root and returns an
error: there is no uncontained fallback or breakaway permission. A stable
Rust/Tokio-compatible thread snapshot is filtered to the newly spawned live
child, whose process handle/PID remains held; unrelated processes/threads are
never resumed or terminated.

Normal root completion closes the lifetime job before collecting the remaining
output. A bounded command cannot leave a child holding its pipes open. Timeout,
future cancellation or runtime loss drops the readers, root and job handle; the
OS terminates the associated descendants. The already-cached `windows-sys`
0.61.2 dependency provides typed Win32 bindings; no new version/toolchain/script
or model is downloaded.

## Observed contained cleanup (subsequent increment)

On Windows, normal `process.exec` completion now requests termination through
the exact owned unnamed Job handle and waits for its native accounting to report
zero active processes before returning. No PID-selected kill, shared-service
control, administrator elevation, duplicate Job handle or new worker is added.
Observation uses the existing operation deadline, including pipe collection;
an expired deadline cannot start the explicit termination request, and a failed/
late query cannot claim observed cleanup. Dropping the owner still retains the
existing kill-on-close fallback for cancellation/runtime loss.

This uses Microsoft's [owned-job termination API](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-terminatejobobject)
and [Job accounting query](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-queryinformationjobobject),
including the native [active-process count](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information).
The root's original success/exit code and bounded stdout/stderr remain intact.
Successful completion includes `process_tree.mode:"windows_owned_job"`,
`empty_observed:true`, and `external_effects_stop_confirmed:false`. Non-Windows
keeps root-child-only behavior and reports `empty_observed:null`, not a tree proof.

The observation covers processes associated with that owned container, not
filesystem/network effects, work delegated to outside services, a security
sandbox or remote stopping. Native deadline/permission/cancellation/restart paths
still retain unknown consumed grants; no replay, migration, extension or success
is inferred. The grant timing advertisement remains `rpc_future_only`.
This is a prerequisite for supported stop acknowledgements, not the remote
stop command/protocol or full native/model/plugin termination acceptance.

Native unit coverage holds live root/child observer handles while
the owned Job reports empty, checks that a second owned tree stays alive, and
checks that an already-expired observation deadline does not explicitly terminate
a live tree. Normal and unsuccessful completion retain their original exit
status. Public `process-lifetime-5YZ9Un` passes the seven existing typed reviewed-
command cases with the new normal/failed-exit metadata assertions.

The broader public pipeline regressions `local-pipelines-EShnQI` and
`local-pipelines-wMgqsj` fail in recurrence. Read-only inspection retains one
completed automatic job, then a second job with zero issued attempts: respectively
an interrupted job (`job_unresolved`) and an approved job (`launch_failed`). Logs
report a stale/withdrawn selected provider and slow journal checkpoints of about
3.7 and 6.1 seconds. Neither run is pipeline acceptance, nor proof of the complete
cause. No freshness window, timer allowance, replay rule or production permission
was relaxed. Timer/heartbeat responsiveness remains an open release gate.

This follows Microsoft's [Job Object lifetime and child association rules](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects),
[assignment requirements](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject)
and [thread resume semantics](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-resumethread).

## What this does not prove

### Executor retirement before final grant checkpoint (subsequent increment)

The shared consumed-grant executor now scopes its executing future so that it
drops before the final synchronous journal save begins. Previously, deadline or
permission failure could enter that save while the pending operation still owned
its process tree; a contended journal lock therefore delayed cancellation cleanup.
The same ordering fix covers Python/peer/pipeline futures through AcceptedGrant,
without adding another runner, approval system or grant outcome format.

The regression first holds the real journal mutex, runs work on another runtime
thread and observes retirement before releasing storage. Five cases cover normal
success/failure, deadline, policy loss and native Windows parent/child cleanup.
The native case retains live process handles and observes both exit while the
receipt remains accepted and the final writer remains blocked. It never kills a
PID-selected unrelated process. Storage is released before assertions/join, even
against the failing baseline. Exact original permissions are restored in a fresh
engine when checking that the consumed grant still refuses replay.

The 25caca2 baseline reproduces three delayed-cleanup failures; normal outcomes
pass. After the fix, all five pass. Retained evidence is in
.validation/grant-cleanup-before.log and .validation/grant-cleanup-after.log.
Public process, Python-budget/bridge, finite-pipeline and permission/node-command
regressions pass on fresh headless binaries; exact fixture paths are retained in
[status](STATUS.md). Both workspace/all-target feature modes pass 860 tests plus
one ignored each. Final headless apps/helpers build and match the evaluated
binary hashes; no fixture app/helper remains running.

Dropping an adapter future requests its existing cleanup; this is not a new
physical stop proof, rollback, recovery or migration authority. A fast final save
can still precede OS exit observation, and outside services or already-dispatched
OS effects can outlive their waiter. A synchronous operation blocked inside its
own poll is also not made preemptible by this scope change. The subsequent
[exact grant stop path](GRANT-STOP.md) now delivers durable intent and signals
the common executor. It still does not acknowledge universal immediate physical
termination. Do not interpret Cancelled tracking as proof every remote effect
stopped.

A Job Object is lifetime containment, not a security sandbox. It does not undo
filesystem/network/device effects, contain work delegated to an external service
or guarantee a hostile program cannot act with the user's permissions. Microsoft
documents exceptions such as children created through WMI. No CPU/RAM/bandwidth/
power limit or anti-malware/signing acceptance follows from this change.

Grant deadline/permission loss still returns retained `unknown` evidence and
refuses replay. OS cleanup is not a fabricated successful grant, recovery proof,
remote stop acknowledgement or authority for migration/continuation. General
native stop/result protocols remain roadmap work.

The original increment covers Windows `process.exec`. The subsequent unreleased
[selected-GGUF lifecycle](MODEL-LIFECYCLE.md) also uses this typed native tree
ownership for model-server startup and explicit load/unload/cancellation.
Intentional persistent application launch/control, Python plugins, camera/audio
and other adapters still need separate measured lifecycle work.
Non-Windows keeps existing root-child kill-on-drop behavior;
there is no non-Windows descendant-containment claim. Public reviewed-process
acceptance is now covered below; full physical two-PC, suspend/disk/power faults and versioned release
acceptance remain required. Stable 0.4.0 is not claimed.

## Validation

The focused Windows suite passes seven entries: six actual native scenarios plus
one test-only helper. They cover refusal before resume without execution,
independent job isolation, root completion with a descendant holding inherited
pipes, process timeout, cancelled execution future, and a granted deadline that
observes both root/child exit but retains persisted unknown effects and rejects
replay. Tests hold real process handles to observe termination, not reused PIDs,
and touch only their own temporary files and children. Test helpers are absent
from shipping binaries; no production opt-out is added.

Full workspace/feature-mode and public regression results are recorded in
[client acceptance](CLIENT-ACCEPTANCE.md). No installed node, published release/
feed, browser assets or model files are changed by this source increment.
Final both-mode suites pass 519 tests and the fresh default workspace executable/
example build passes; earlier failed link evidence is retained separately.

## Public reviewed-path acceptance (subsequent increment)

`tools/test-process-lifetime.mjs` runs two separate pinned-TLS logical nodes on
one PC, without models or a dashboard controller. The origin's process feature
stays disabled; only the explicitly disclosed executor allows the isolated
native example. Submission/compilation and owner approval create no processes.
Execution uses the existing public reviewed grant path with exact operation,
parameters, immutable origin, fixed executor and one retained attempt.

Six scenarios check normal root completion with a child holding inherited pipes,
local process timeout, grant deadline, saved local permission revocation,
executor restart and coordinator restart. A separate read-only native observer
opens handles while both processes are alive, verifies their recorded creation
times, retains those handles and observes both exit signals. It does not kill
by PID or infer termination from missing/stale PID listings. See Microsoft's
[process timing API](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes)
and [held-handle wait semantics](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject).

Only normal completion reports `completed`; the other cases retain `unknown`
effects, with exact receipt inspection leaving one attempt and unchanged native
identities. The observed exit is fixture evidence, not a new production remote
stop acknowledgement, successful grant or permission to replay/migrate.
The example is a development helper, not part of installed bundles or a new
shipping invocation backdoor. Both root and child have finite safety lifetimes.

Build the helper with `cargo build -p eefn --example process-fixture --locked`,
then run `node tools/test-process-lifetime.mjs` against fresh default role binaries.
The test changes only isolated fixture permissions/configuration and its own
temporary files/processes; it never updates installed nodes or changes ordinary
production permission/approval rules. Native exit observations do not establish
model/plugin/media lifetime, every hostile program/external service, physical
two-PC, full resources or installer/security acceptance.

Final public fixture `process-lifetime-TpFkkR` passes all six cases and exact
grant parameter/origin/executor checks. Earlier `process-lifetime-1ivWWA`
passed the six cases before the additional grant-scope assertions;
`process-lifetime-BHKIlg` passed five cases, and `process-lifetime-1xzKba`
passed the initial three. The first `process-lifetime-5O7xtV` run failed on a
closed diagnostic keep-alive socket during restart; it is not a pass. Only
read-only observation predicates now tolerate transient fetch failures; no
mutation, approval or execution is implicitly retried. Final workspace suites
still pass 519 tests in both feature modes with the new helper included.

The strengthened dashboard-free-binary fixture `process-lifetime-pmU8lz` also
passes all six cases. Its held-handle observer adds monotonic elapsed timings:
permission revocation and both restart cases must close the processes within
2000 ms of live observer admission, well before their 10000 ms grant deadline.
This distinguishes lifecycle cleanup from merely waiting for lease expiry.
No production deadline or authority rule is relaxed. The preceding dashboard-free
attempt `process-lifetime-EH59gk` exited 1 without a normal result/failure report;
no fixture processes remained. It is not a pass or a diagnosed/fixed failure.
A traced rerun with private bounded exit diagnostics passing does not establish
that abrupt exit's cause. Default role binaries are restored after this check.
Final default-binary `process-lifetime-qNeb1d` passes the same strengthened six
cases after restoration. Final both-mode suites remain 519 tests, with formatting,
JS syntax, diff and backend guard passing. Installed artifacts remain unchanged.

## Typed process authoring and unsuccessful exits (subsequent increment)

`eefn requests submit-process` now builds that same exact concrete request from
literal program/argument/working-directory/timeout options, without a JSON file
or model. It only submits/compiles for review; owner approval, grants, local
permissions, fixed executor and original source/receipt checks remain. The public
lifetime fixture uses this adapter and inspects its exact source reuse before
approval to establish no execution. [Options and scope](EXPLICIT-REQUESTS.md).

The fixture additionally exposed a real status bug: `process-lifetime-SWGvia`
observed native exit 17 with `data.success:false`, but the grant marked success
and eef marked the job completed. That failed run is not acceptance evidence.
The node now uses the existing completed-but-unsuccessful report shape with
`process_exit_unsuccessful`, retaining bounded exit/stdout/stderr evidence and
`success:false`. eef's existing unsuccessful-task path records `failed`, not
completed, and does not replay the command. Other capability payloads' unrelated
`success` fields are not reinterpreted. No schema or permission is widened.
Matched-grant confirmation also rejects a contradictory native report claiming
success while its process payload says failure. Older/forged contradictory
records are retained and fail validation/recovery rather than being rewritten,
silently repaired or used as successful automation evidence. Preserve journal
backups; stricter evidence checks are not automatic old-record remediation.

Completion means the bounded invocation returned; it is distinct from the
program succeeding. A failed command may already have caused effects; neither
failure nor observed process exit undoes them or authorizes retry, migration or
refunding an automation allowance. Deadline/permission/restart uncertainty still
reports `unknown`. The new failed-exit case retains one attempt and original
native identities under exact receipt inspection.

Final `process-lifetime-oSCVSl` passes all seven typed-source/public-path cases
after the false-success confirmation guard. It records the failed command's
grant success/error/exit fields and failed job status explicitly in its results,
not merely a generic completed receipt. Final both-mode suites pass 529 tests;
initial failed test/compile runs and the reproduced false-success defect remain
separate from that acceptance evidence. No published/installed artifact is updated.
