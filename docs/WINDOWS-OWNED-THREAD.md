# Process-local lookup for an owned suspended child

An unpublished native Python/pipeline diagnostic recorded containment/resume
durations between roughly 200 ms and 2.25 s. Process creation itself was usually
much shorter. The pipeline failed even its first positive synthetic capture
under the unchanged two-second bridge budget; it remains failed. These timings
locate substantial synchronous overhead, not every cause of that failed result.

The old lookup used a Toolhelp thread snapshot and walked every thread on the
PC to find the primary thread of a newly suspended child. A separate three-sample
native probe compared that with a process-local, thread-ID-only snapshot against
the same owned child. Both identified exactly its known primary thread. The
global walk inspected 5,656 threads and took 93–124 ms; the process-local query
took 0.22–3.73 ms on this host. The probe never resumed its child and observed its
termination through the retained process handle. These are diagnostic samples,
not a general latency guarantee or application acceptance.

The candidate keeps the existing ownership sequence:

1. Create and read back the unnamed kill-on-close job and optional memory limit.
2. Create our child suspended; assign it to that job before any user code runs.
3. Capture **only that child's thread IDs**, require exactly one nonzero owned
   thread and a complete walk, and release the snapshot and walk marker.
4. Open that thread, independently verify its process identity, then require
   `ResumeThread` to return the expected previous suspend count of one.

No system-wide scan, memory/address-space clone, context capture, handle-table
capture, injection, remote target, new authority or elevated startup is added.
Unexpected ownership, missing/ambiguous threads, incomplete enumeration, cleanup
failure or changed resume count refuses execution. The caller retains the job
and root kill path on refusal; there is no uncontained or legacy-scan fallback.
No grant, Python deadline, result budget or existing native assertion changes.

The APIs return Win32 error codes directly, so failures must not be diagnosed
using a stale `GetLastError`. The snapshot is captured into the caller: free it
with `PssFreeSnapshot(GetCurrentProcess(), ...)`, not `CloseHandle` or the target
process handle. The walk marker has its own free function. Owned wrappers cover
early returns; successful cleanup is checked before resume. See Microsoft's
[capture API](https://learn.microsoft.com/en-us/windows/win32/api/processsnapshot/nf-processsnapshot-psscapturesnapshot),
[walk contract](https://learn.microsoft.com/en-us/windows/win32/api/processsnapshot/nf-processsnapshot-psswalksnapshot),
[thread entry](https://learn.microsoft.com/en-us/windows/win32/api/processsnapshot/ns-processsnapshot-pss_thread_entry)
and [snapshot ownership](https://learn.microsoft.com/en-us/windows/win32/api/processsnapshot/nf-processsnapshot-pssfreesnapshot).
These APIs require Windows 8.1 or later; supported Windows 10/11 builds already
meet that requirement. Other platforms retain their existing behavior.

Four pure selector tests cover complete single-thread success and empty, zero,
foreign, duplicate/multiple and partial/error cases. Fresh native tests must
still prove startup/refusal, descendant lifetime and memory limits through actual
held handles, then exercise the original Python and pipeline paths. Full Rust,
hosted, physical and stable-release acceptance remain separate. Earlier failures
are not erased or explained away by a later pass.
