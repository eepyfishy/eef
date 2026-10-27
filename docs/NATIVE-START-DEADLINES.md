# Deadline-aware start for bounded owned processes

This is an in-development execution-safety follow-up, not stronger stop proof.
The accepted `f50a5a1` checkpoint keeps the existing `rpc_future_only` grant timing
advertisement and intent-only stop receipts. Neither guarantee is changed here.

Python previously checked its deadline immediately before synchronous process
creation. Windows then created the suspended child, assigned its owned job,
inspected and verified its primary thread, and resumed it without another budget
check. Time spent inside those OS calls could consume the remaining window.
The local `process.exec` timeout also began after that start sequence. Faster
thread lookup reduces observed overhead but does not remove the boundary gap.
This identifies the code path, not the exact cause of earlier fixture failures.

The candidate adds a private deadline-aware spawn entry point. It checks the
existing absolute monotonic deadline before creation and, on Windows, again
after job assignment, complete thread lookup/cleanup and owner verification,
immediately before resume. A refused child stays suspended and contained; the
existing root-kill/job-close failure path retires it. No fallback or larger
budget is used. Non-Windows retains its existing root-only behavior and can only
check before spawn, not promise suspended containment.

The already accepted lease deadline is threaded through ordinary granted calls,
finite pipeline stages and both peer-audio paths. Bounded process/Python adapters
intersect it with their local budget rather than starting a replacement window.
`process.exec` starts its local timeout before command preparation/spawn. Python
keeps its configured budget but adds numeric `effective_budget_ms` diagnostics
for the possibly tighter parent window; start refusal retains existing deadline
failure categories. Configuration, schema/signature prefixes, permissions,
replay/receipt rules, byte/memory limits and approved peer routes do not change.

Other helpers with independent lifecycle authority keep their existing behavior;
this is not automatic cancellation of a shared model server or outside service.
Filesystem/network/device effects, arbitrary synchronous blocking, revocation
between the final check and OS resume, and already queued effects remain outside
a hard real-time or physical-stop claim. A timeout still leaves uncertain effects
and consumed authority; it does not prove rollback or authorize retry/migration.

New tests check minimum-budget intersection, refusal before even spawning an
invalid program, typed/private-error-safe classification, expired Python and
process parent propagation, and native final-gate placement. The Windows oracle
uses only the verified owned primary-thread handle: it briefly increments and
restores its suspend count, requiring the original count to be one. A refusal
then observes exit through a retained root handle; the positive control starts
and retires its real root/descendant tree. This avoids sleep-dependent first-poll
assumptions and does not resume an unrelated process. Focused/full/native/hosted
acceptance is pending; no release or installed/physical-node update is implied.
