# Deadline-aware start for bounded owned processes

This execution-safety follow-up is accepted at development checkpoint `a356aa7`,
not as stronger stop proof. The existing `rpc_future_only` grant timing
advertisement and intent-only stop receipts remain unchanged.

Python previously checked its deadline immediately before synchronous process
creation. Windows then created the suspended child, assigned its owned job,
inspected and verified its primary thread, and resumed it without another budget
check. Time spent inside those OS calls could consume the remaining window.
The local `process.exec` timeout also began after that start sequence. Faster
thread lookup reduces observed overhead but does not remove the boundary gap.
This identifies the code path, not the exact cause of earlier fixture failures.

The implementation adds a private deadline-aware spawn entry point. It checks the
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
assumptions and does not resume an unrelated process.

## Verification at `a356aa7`

Seven new tests pass. Both full local Rust modes pass 1,178 tests plus one existing
ignored profiler each; all 37 local native cases pass. The exact
[four-job CI run](https://github.com/eepyfishy/eef/actions/runs/40480350717)
passes, including 31 Windows native suites and the distinct connection-close case.
Separate cached native CPU and default-feature browser/copy/focus/restart checks
pass. Independent reconciliation verifies frozen source/binary/model/runtime
hashes and read-only job journals, without model downloads or replay.

The first independent observer failed because it assumed a fixed number of
harness children: the focused fixture has three extra read-only schedule-show
polls. Its original failure remains retained. A corrected observer verifies the
read-only flags, matching other command counts, two one-attempt jobs and exact
durable receipts in both fixtures, exhausted schedules and unchanged database
hashes. No application test, deadline or authority rule was changed or rerun to
obtain that reconciliation.

This accepts the exact development checkpoint, not current physical two-PC
operation, automatic model onboarding/quality, strong stop, continuous workloads,
production installer recovery, signing/Defender or stable-release acceptance.
No release or installed/physical-node update is implied.
