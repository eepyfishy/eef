# Owned-model job accounting (unreleased)

```text
eefn --json model-lifecycle show
eef --json node model-lifecycle --node NODE_ID show
```

Lifecycle status now includes optional versioned-parent `job_accounting` data for
each selected GGUF runtime. It queries only the runtime's owned native job handle;
there is no global process scan, PID-based lookup, model inference, lifecycle
mutation, file deletion or quota adjustment. The existing runtime/generation and
owned-process lock retain the queried handle while the sample is read. A sample
does not imply that a root is ready/alive, that an operation completed, or that
remote execution is authorized.

## States and units

| State | Meaning |
| --- | --- |
| `available` | Windows job queries returned the fields below. |
| `no_owned_job` | No retained owned-job handle exists for this selection. No zero usage is invented. |
| `unsupported` | A root is owned but this platform has no implemented native job accounting. |
| `unavailable` | The owned-job observation failed or its returned data was invalid. No estimated metrics or raw OS details are substituted. |

An available sample contains `job_commit_limit_bytes` (explicit null when no
eef job ceiling is set), `peak_job_commit_bytes`, `active_processes`,
`total_user_cpu_100ns` and `total_kernel_cpu_100ns`. Values come from Windows
[extended limits](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_extended_limit_information)
and [basic accounting](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information).
CPU totals use 100-nanosecond ticks and include associated processes that have
terminated. Active process count includes the job's members, not only eef's
root/known application children. Peak committed memory is not current commit,
working set, total node RAM, VRAM or a model resource estimate. Parent/other OS
limits can be stricter than the job's configured ceiling.

The two queries are **not an atomic resource snapshot**. CPU rates/percentages
are not calculated, a wall-clock freshness/lease is not added, and these values
are not fed to quota admission, routing, unload decisions or heartbeat metrics.
An unloaded selection reports no owned job; this is not proof of physical memory
release or absence of external effects/services. Reload starts a new owned job,
so samples from different generations must not be treated as one cumulative run.

## Wire compatibility and validation

Older lifecycle snapshots without `job_accounting` remain accepted; an optional
null field also supplies no observation, never zero usage. For a supplied sample,
known states, required available metrics, explicit limit/null,
unsigned values and bounded MiB-aligned ceilings are checked. Unknown/extra
metrics, missing/negative values and a no-owned-job claim with an owned PID are
rejected. Empty-struct tagged states deliberately reject extra metrics; serde's
unit variants were found to accept them in the first failed test and are not used.
These are structural checks, not remote kernel attestation or proof that a node
reported truthful usage. No registration or execution authority is derived here.

The first full regression run also exposed a legacy assertion comparing entire
lifecycle snapshots while a process was still running. CPU/peak/process metrics
can change without any denied command changing residency. The test now compares
all lifecycle/identity fields exactly and validates both observations separately;
the authorization, wait/recheck and inference-pin assertions remain intact.

## Verification (2026-10-14)

- Fresh default and dashboard-disabled workspace/all-target suites each passed
  **660 tests**, with one ignored. The native contained-root/child test queries
  the same owned job twice, verifies the 128 MiB configured ceiling, observes
  positive peak commit and at least the two known live processes, and confirms
  that querying does not retire them. It then observes their exit after closing
  the owned job. The job member count is not a PID census or identified-child list.
- Fresh binaries/examples, formatting, diff whitespace, changed JavaScript
  syntax and the backend/browser source-boundary check passed.
- `model-lifecycle-EfdxGM` passed the public local/coordinator lifecycle commands
  over pinned TLS using synthetic model servers. Read-only accounting preserves
  runtime/generation/root identities before remote management is enabled.
  Extra metrics, invalid ceilings and an omitted available limit are rejected
  without retry/fallback; old snapshots omitting the additive field are accepted.
  Targeted unload/reload, active-use pins, cancellation, revocation rechecks and
  exact historical no-replay checks also pass.
- `real-model-D8bLLw` reused the verified experimental Qwen 3.5 0.8B artifact
  without download/copy and passed reviewed real CPU inference plus lifecycle,
  idle/on-demand and memory-ceiling failure/reset/restart checks. Its warm-idle
  job sample reports peak commit **1,004,474,368 bytes**, two active members,
  31,250,000 user and 14,062,500 kernel CPU ticks, and an explicit null job
  ceiling. These are one job's observations, not current/total RAM or a promised
  cost. Unload reports `no_owned_job`; no artifact is deleted. Fixture processes
  exited. This does not approve the model as the automatic interpreter default.

Both integration fixtures are isolated localhost tests, not physical two-PC or
release acceptance. Earlier failed tests are retained/documented above; they are
not erased by the fresh passes.

## Remaining scope

This supplies scoped observations alongside the existing [job-commit ceiling](MODEL-MEMORY-LIMITS.md).
Current/aggregate RAM, VRAM and CPU-rate measurement/admission, bounded background
sampling, scheduler integration, cross-platform enforcement, physical two-PC
acceptance and the full stable release roadmap remain open. No installed node,
model default, permission, release or automatic update is changed.
