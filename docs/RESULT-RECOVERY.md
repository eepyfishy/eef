# Explicit result reconciliation (unreleased after alpha.6)

An interrupted reviewed job can now settle its last uncertain attempt using the
exact executor's retained **completed** report. This is an explicit command,
separate from read-only `requests result`, and is not execution or continuation.

```powershell
eefn requests recover-result JOB_ID --revision REV --grant-id GRANT_UUID --operation-id RECOVERY_UUID --json
eef requests recover-result JOB_ID --revision REV --grant-id GRANT_UUID --operation-id RECOVERY_UUID --json
```

Inspect `jobs get` first for the interrupted revision and uncertain grant. Choose
a distinct UUID for this decision and retain the entire command. The node command
uses the running origin node's existing authenticated connection. The EEF command
is for its local owner. Caller-supplied reports, completion flags, permissions,
issuer identity, replacement grants and retry fields are rejected.

EEF validates immutable origin, approved plan, original grant and journal before
reading the exact executor. Retrieval uses the existing independently
authenticated pinned-issuer control query and full issuance digest, including
the original session; it does not grant execution authority to a new session.
Admission is checked again under the job runner/shutdown gate before committing.
No lock spans the remote read. A changed revision, live runner, shutdown,
abandonment or concurrent winning decision prevents a new checkpoint.

Only a bounded, checksum-validated retained `completed` report that confirms the
full original grant can settle tracking. A completed error is a **failed** task,
not success. Legacy metadata, an accepted receipt, missing output or a retained
`unknown` report cannot settle it. Missing evidence never means no effects.
Independent node/EEF wall clocks are not compared to each other.

In-process cancellation/disconnect drops the accepted node runtime. It now
best-effort checkpoints a null-output `unknown` report
(`grant_runtime_dropped_effects_unknown`), preserving consumed identity and
releasing unfinished-result reservations when that write succeeds. It does not
claim an external backend/process stopped, and cannot be reconciled as completed.
Process crashes or failed writes may still leave only accepted metadata. A final
checkpoint failure is not retried by this cleanup path.

One atomic job checkpoint stores a separate per-attempt recovery record with
the exact command, actor, local decision time and original bounded node evidence.
The grant, immutable request context, assignment, attempt count, real wire ACK
and original missing/unknown report are preserved. Losing an ACK does not create
a synthetic one. Task/result metadata reflects the explicit reconciliation;
routine job views expose only a redacted recovery receipt/checksum. Explicit
`jobs output` can read recovered text. Full retained evidence is private job
data, included in owner inspection and database backups.

Success completes the job only when all its tasks are completed. Any unissued
steps remain held/interrupted, never automatically dispatched; explicit stop
may cancel unissued work after issued uncertainty is resolved. Failed evidence
fails the job and cancels unissued work. This policy is not implementation or
acceptance of compound camera-to-VLM wiring: the current concrete compiler still
rejects implicit image wiring. Continuous workloads and a separately authorized
safe-continuation protocol remain open.

Repeating the **exact** command with the **same actor** inspects its saved receipt,
without fetching evidence again or stamping another revision, even when the
executor is offline. Another origin, owner/origin actor change, changed revision
in the command or replacement operation ID cannot reuse/replace that decision.
Restart preserves recovery without installing a runner. Late wire completion
cannot overwrite the recovered record or revive work. Earlier binaries reject
the new recovery field rather than silently interpreting it: stop old runtimes,
back up data and do not downgrade/overlap a recovered job journal.

The node sends one command only. A lost/malformed/error reply is unconfirmed;
unless explicitly known not sent, its mutation outcome is unknown. Inspect the
retained job and reuse only the exact saved receipt after uncertainty. No
automatic retry, legacy RPC/chat fallback, lease renewal or model call occurs.
Recovering final evidence proves neither rollback, exactly-once external
effects, model correctness nor strong process/backend termination.

Both guarded local APIs use `POST /api/commands/requests/recover-result`:

- Node: `{schema_version:1,expected_node_id,command:{id,expected_revision,grant_id,operation_id}}`.
- EEF owner: `{schema_version:1,expected_runtime_id,command:{id,expected_revision,grant_id,operation_id}}`.

Authenticated node submissions use `requests.recover_result`; the operation ID
must equal the authenticated request correlation ID. Existing local-owner and
cross-site guards apply. Public recovery does not accept raw cached evidence.

The existing job quota also bounds recovered evidence. Interrupted jobs have
already released execution reservations; a full/lowered journal or failed write
may prevent recovery. The source remains unchanged, the node cache remains
independently inspectable, and no execution is retried to recreate output.
Logical quotas do not guarantee physical disk space or rollback overhead.

This recovery increment did not itself retire legacy task/chat/rule/RPC execution;
the subsequent [execution boundary](EXECUTION-BOUNDARY.md) fences those paths.
Neither increment implements
strong native termination, automatic reconciliation, remaining-step continuation,
continuous pipelines, signing or physical two-PC acceptance. No release version,
installed app, physical node or stable-only update feed changes here.

The headless `tools/test-request-review.mjs` fixture uses built-in `node:sqlite`
(validated with Node.js 24.15.0). It injects/removes an exact-job SQL trigger only in
its isolated temporary database, to fail the coordinator's completion/recovery
checkpoint without changing production APIs or installing a test backdoor. This
tests local checkpoint loss, not physical two-PC behavior or a dropped TCP final
response. A separate real local disconnect test expects uncertainty, not success.
