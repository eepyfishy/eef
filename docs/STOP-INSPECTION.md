# Read-only stop evidence (unreleased after alpha.6)

Stop delivery and inspection are different operations. A lost stop reply does
not prove failure, and inspecting retained intent must not send another stop.

    eefn requests stop-receipt JOB_ID --grant-id GRANT_UUID
    eef requests stop-receipt JOB_ID --grant-id GRANT_UUID
    eefn grants stop-receipt --id GRANT_UUID

The first command uses the running originating node. The second uses the local
coordinator owner's command service. Both select the original issuance from the
existing plan, require its origin/owner authority and ask the exact executor for
retained evidence. The third reads the executor's local journal without starting
a node runtime or connecting to a coordinator. Add --json for structured output.
Commands require the original IDs; they do not search or guess another attempt.

## Evidence, not termination or recovery

An inspection reports missing or retained evidence. Retained evidence binds the
original grant digest, issuer, session, executor, job, task and attempt, and gives
the original stop timestamp and whether consumption already existed. It always
reports execution_stopped:false and external_effects_stopped:false. Historical
inspection deliberately omits cancellation_signalled: that live notification
result was never persisted, so a reader cannot reconstruct it truthfully.

The query itself reports read_only:true, execution_requested:false,
stop_requested:false, automatically_retried:false and reconciled:false. A nested
retained evidence.stop_requested:true describes historical intent, not a new
action. Reading does not revise the coordinator's attempt state or replace its
saved delivery reply. An uncertain delivery stays uncertain in coordinator
tracking, even if a separate inspection finds retained intent.

Missing evidence is not proof that no stop or work was accepted. Unavailable,
unsupported, corrupt or mismatched evidence is unconfirmed, not missing. No
query executes work, notifies a stop watcher, repairs/prunes storage, changes
permissions, renews a grant, reconciles tracking or requests a restart/update.
For actual stop behavior and its limitations, see [exact grant stop](GRANT-STOP.md).

## Reconnection and compatibility

The separate grant_stop_read schema-v1 advertisement promises
read_only_intent_evidence. It leaves execution_grants and grant_stop unchanged.
Older nodes without this advertisement are unsupported; there is no ordinary
RPC or mutating stop fallback.

Network inspection requires current pinned credential TLS authenticating the
same original coordinator issuer and exact executor node. Unlike mutating stop,
the connection session may differ from the historical session. The response
must still preserve and match the entire original scope, including that original
session. A reconnect does not retag, rearm or authorize stopped work. A different
coordinator key/database does not acquire the original authority or global
history merely by learning a grant ID. Local executor-owner inspection remains
available without the network; a missing local record cannot invent a scope.

## Storage and resource boundaries

Network reads use the existing bounded receipt-control admission pool. Blocking
callbacks retain the permit even if their async waiter disappears. Reads use one
database snapshot, validate the root owner/version, consumption and adjunct
schema, bound evidence before materializing strings, and never initialize or
repair an adjunct. Corrupt metadata cannot be silently treated as no record.
The offline adapter opens the existing journal read-only; a missing file stays
missing. This adds no runner, store, model invocation or dependency.

SQLite locking/full-disk failures can make inspection unavailable. Evidence is
local to retained node storage, not a replicated audit service. It does not
prove physical device buffers drained, external services stopped, rollback,
hard-real-time cancellation or continuous media safety. Physical two-PC,
default-model/bootstrap, installer/Defender/signing and stable-release gates
remain open.

## Native stack regression

Adding the optional control branch exposed a Windows main-thread stack overflow
in the synthetic finite-audio path. Native exit code 0xc00000fd and the node's
stack-overflow diagnostic distinguish this from a permission or journal refusal.
The handler now lives in a separate boxed control future, and endpoint sessions
are heap-owned instead of enlarging the caller's inline connection/retry frames.
No OS thread-stack setting, permission, deadline or acceptance assertion changed.
A regression bounds connection/retry future sizes; the unchanged full native
pipeline fixture passes after the fix. This is evidence for that exercised
default-stack Windows path, not a universal stack/resource guarantee.

## Verification

Both locked/offline workspace/all-target feature modes pass 886 tests plus one
ignored profiler each. Thirteen new regressions cover reader/report/CLI behavior,
bounded connection frames and fixed-code early peer diagnostics. The native
process-lifetime-TX7mne fixture checks all three inspection adapters, missing and
retained evidence, wrong-origin refusal, original scope after reconnect, byte-
stable node journals and coordinator tracking, and a fresh local CLI with the
executor runtime absent. The previous nine process-lifetime scenarios also pass.

The unchanged local-pipelines-UTIKFN, python-policy-zu5yVa,
python-bridge-IJhd5z, permission-commands-2DuR28, node-commands-fg458E,
node-credentials-Hbju03 and model-downloads-gUrt9d public suites pass against the
same evaluated native build. They use isolated logical nodes, synthetic media
and providers/plugins, not installed/remote nodes or physical two-PC media.
Final apps and both fixture helpers match the evaluated hashes; no visual UI,
installed application, model, version, installer or update policy changed.
