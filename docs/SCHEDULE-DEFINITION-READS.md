# One-call definition indexing for schedule inspection

The owned-thread candidate passes its four selector tests, thirteen existing
lifetime tests, fresh headless build and native lifetime/Python suites. Its full
pipeline fixture still fails at the recurring schedule's original 45-second
observation deadline. The journal retains two completed, successful parent jobs
with acknowledged grants and durable executor receipts; the schedule remains
`in_flight`, with two claimed runs. No replay or repair is performed against that
failed fixture. Earlier failed stages remain distinct evidence.

Numeric observations in this fixture show a maximum containment/resume duration
of 5 ms across 45 starts, and the original Python pressure/deadline and paired
audio assertions are reached successfully. That is useful local evidence for
the Windows lookup, not full acceptance or proof of earlier failure causes.
Recurring launch/settlement still competes with repeated validated snapshots;
logs show multi-second journal waits and hundreds of milliseconds of validation.

A schedule snapshot previously read and validated each policy's definition in
the point-policy reader, then read each distinct definition again to build its
history view. Multiple policy generations repeat the same definition work too.
The candidate builds the dependency index within that **one locked invocation**
and returns it with the fully checked policy rows. Each distinct definition is
read through the existing definition validator once for that view. Every policy
still uses the original authorization/compiler/scope, row identity/reserve and
generation-history predicates. Point reads are retained and share those exact
checks; ordinary policy-list callers retain their existing reader.

The index is local, not stored in the journal/runtime or reused across calls,
writes, admission or commit fences. No unrelated policy is skipped. Snapshot
schedule/trigger predicates, launch/settlement writes, clocks, grants, retries,
quotas and original fixture assertions/budgets remain unchanged. This removes
duplicate definition reads, not all repeated validation and not all contention.

Three focused tests require one actual definition-loader call per distinct
dependency across multiple policies/generations, parity with point readers,
zero inspection writes, fresh refusal after a definition changes, and identical
identity/scope/reserve/generation/future-record refusals. These are correctness
and work-count checks, not latency guarantees. Focused, full/native and hosted
acceptance remain pending. A later pass cannot erase the retained failed run.
