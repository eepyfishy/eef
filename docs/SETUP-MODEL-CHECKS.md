# One-shot model check after setup (in development)

An owner can separately consent to one fixed inference once an exact node,
backend and model have applied setup. The backend waits and uses the existing
node-authored source, owner approval and durable execution grants. The dashboard
or CLI submits one command; neither drives setup polling or execution.
This is not complete automatic onboarding or an approved project default.

```text
eef node setup-model-check --node NODE arm --backend ollama --model-id MODEL --allow-inference --wait-seconds 120
eef node setup-model-check --node NODE inspect --operation-id OPERATION_UUID
eef node setup-model-check --node NODE cancel-wait --operation-id OPERATION_UUID
```

`arm` prints its operation UUID and also accepts an explicit `--operation-id`.
Keep that ID if the reply is lost. The API is local-owner POST
`/api/commands/node/setup-model-check`, with a 4 KiB request limit, exact current
coordinator runtime and strict versioned command bodies. CLI clients resolve
only that coordinator runtime; they do not inspect node setup, retry a lost
mutation, run a local controller or fall back to older commands.

## Separate consent and exact selection

Download, activation and restart preferences do not authorize this inference.
`arm` requires explicit inference consent, a node ID, exact `ollama` or `llamacpp`
backend/model and a 1-300 second controller window. It permits only the existing
fixed greeting check: no caller prompt, tools, files or widened inference limits.
The check itself never selects or downloads a model, enables node permissions,
restarts anything or approves model quality/default candidacy.

While waiting, eef makes bounded read-only readiness observations. Missing,
unapplied, unconfirmed, privately unpaired or locally unconsented setup is not
authorization. A different model cannot substitute for the consented one. The
actual node runtime is resolved only from confirmed applied setup before launch;
the ordinary model-verification preparation then pins and rechecks that exact
runtime, model and role. Node runtime changes while merely waiting do not retag
an already-issued grant or rearm old work.

## Durable identity, cancellation and restart

Consent is immutable in the existing job database before a worker is admitted.
Its operation ID, node, model and time budget cannot be replaced. Identical reuse
only returns retained state, including after cancellation, timeout, worker loss,
restart or finished-job removal. Existing model-verification intents also prevent
arming that old operation again. There is no resume/retry command.

Only the original live coordinator worker owns a monotonic deadline. Workers
are not reconstructed from saved records on startup. Lost waiting ownership is
reported as interrupted; lost launching ownership is unknown, not safely failed.
Shutdown retires the owned coordination tasks before normal job/transport
shutdown. Existing admitted-work uncertainty and no-replay behavior remain.

`cancel-wait` races atomically with the one launch claim. A confirmed successful
waiting cancellation prevents that consent from launching. After launch it
returns `waiting_cancelled:false`; it is not cancellation or termination of
admitted inference. Inspect the original verification/job and use existing job
stop controls where appropriate. Those controls still do not prove universal
physical stop, rollback or outside-service termination.

The controller deadline includes readiness observation and result inspection.
Expiry before a launch claim is distinct from unknown effects after a claim.
Dropping its async future does not make synchronous OS/storage work preemptible
or stop a previously issued grant. Uncertain effects remain unknown and are never
automatically retried. Read-only polling after the first start inspects retained
evidence only; it does not repeat preparation, approval or execution.

## Status, quotas and compatibility

Plain output distinguishes waiting, returned text, no text, failure, cancellation,
expiry, interruption and unknown effects. A finished record contains only a
bounded historical summary and IDs, not private model prose or a complete live
health/quality claim. Removing a finished job does not erase consent or restore
permission to infer. Read-only inspection never reconciles or resumes work.

Each record reserves 4 KiB, including future outcome metadata, against the shared
`jobs.max_count` and `jobs.max_bytes` budgets. Existing jobs, automation records
and model-check intents count those reservations too. Full/corrupt storage fails
closed; there is no automatic pruning, quota increase or identity reset.
The additive records have strict schema/identity validation and bounded reads.
There is at most one active controller per node, and two across the coordinator
by default. Advanced owner configuration `setup_model_checks.max_active` permits
1-8; changing that limit requires a coordinator restart. Omission never arms any
check, and older configurations remain inert. Malformed optional limits cannot
silently enable automation or widen the cap.

This does not introduce another workload runner or node RPC execution bypass.
Validation of the new code, public command integration, full regression and
hosted acceptance remain pending. Physical two-PC, automatic default/bootstrap,
continuous work/strong stop, installer recovery/signing/Defender and stable
v0.4.0 requirements remain separate and open.
