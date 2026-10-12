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

This follows Microsoft's [Job Object lifetime and child association rules](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects),
[assignment requirements](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject)
and [thread resume semantics](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-resumethread).

## What this does not prove

A Job Object is lifetime containment, not a security sandbox. It does not undo
filesystem/network/device effects, contain work delegated to an external service
or guarantee a hostile program cannot act with the user's permissions. Microsoft
documents exceptions such as children created through WMI. No CPU/RAM/bandwidth/
power limit or anti-malware/signing acceptance follows from this change.

Grant deadline/permission loss still returns retained `unknown` evidence and
refuses replay. OS cleanup is not a fabricated successful grant, recovery proof,
remote stop acknowledgement or authority for migration/continuation. General
native stop/result protocols remain roadmap work.

This increment covers Windows `process.exec` only. Intentional persistent
application launch/control, model-server lifetimes, Python plugins, camera/audio
and other adapters retain their own existing behavior and need separate measured
lifecycle work. Non-Windows keeps existing root-child kill-on-drop behavior;
there is no non-Windows descendant-containment claim. Full physical two-PC,
public reviewed-process planning, suspend/disk/power faults and versioned release
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
