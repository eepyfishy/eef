# Retained executor results (unreleased after alpha.6)

Granted operations now save bounded final reports/output in the existing node
grant journal **before** sending completion. This supplies independent evidence
after a lost reply or failed coordinator checkpoint, without repeating work.

```powershell
eefn requests result JOB_ID --grant-id GRANT_UUID --json
eef requests result JOB_ID --grant-id GRANT_UUID --json
eefn grants result --id GRANT_UUID --json
```

The input-node command uses its running connection and discovered local API.
EEF permits only that immutable origin or its local owner, selects the retained
issuance and queries its exact executor. Local `grants result` works offline,
starts no runtime/model/listener and creates no file. No command retries work or
falls back to ordinary RPC, another executor or local adapters.

Remote `grant_result` is a distinct reserved-control query. It requires independent
credentials/pinned TLS, the current issuer equal to the original issuer, exact
executor and complete issuance digest. EEF checks the full original grant,
including old session, job/task/attempt, plan, result limit and scope digest.
Session replacement permits inspection but never revives an old lease. Executors
advertise `retained_results`; missing/false advertisements deny without fallback.

Availability is `retained_final_report` (inspect terminal state/success/error),
`no_final_checkpoint` (consumed but potentially running/interrupted/uncertain), or
`legacy_metadata_only` (no saved output/success evidence). Migration never invents
results. Responses set `execution_requested`, `automatically_retried`, `reconciled`
to false. EEF state/revision remains an unchanged snapshot even when node evidence
reports completion. Explicit journal reconciliation is still open. Completion is
not proof of external termination, rollback, exactly-once effects or model truth.

Final reports may contain private text/results. They are stored in plaintext
beside node configuration and included in backups; keep that directory private.
This is not encrypted or tamper-proof storage. Parameters/original prompts/media
inputs are not separately copied, but output may itself contain sensitive input.
Routine receipts, job listings/status and diagnostics do not expose cached output.
Outputs are not automatically logged or added to ordinary UI/job views.

The existing `CONFIG.grant-receipts.sqlite3` migrates additively from schema 1 to 2,
retaining consumed identities/digests/states/timestamps with no fabricated report.
Owner/version/records are validated before migration. Damaged/future journals fail
closed and remain intact, never repaired/recreated. Read-only schema-1 inspection
does not migrate. Older binaries reject schema 2: stop old runtimes, keep backups
and do not downgrade/overlap runtimes sharing the journal.

Admission reserves the result limit plus bounded report/SQLite headroom before
consumption/ACK/effects. Accounting includes all unfinished reservations across
independent handles. Terminal state/report update is one FULL-synchronized SQLite
transaction; that checkpoint releases its reservation.
The checkpoint also saves a canonical SHA-256 report checksum, checked on reads
and startup. It detects changed cached bytes, not malicious-owner/node tampering
when the checksum can also be replaced; it is not a signature or attestation.
Existing configurable receipt-count/journal-byte/result bounds apply, with fewer
grants potentially fitting now that outputs consume the same budget. There is no
unlimited cache or automatic pruning. Deleting/replacing the journal loses replay protection.

Logical reservations/page caps are not physical free-disk guarantees; rollback
journal overhead is additional. A failed final write cannot confirm success: the
earlier consumed receipt remains uncertain without recoverable output or replay.
Oversized output is omitted with `result_too_large`, never retrieved by repeating
effects. The deadline remains `rpc_future_only`, not process-tree termination.

Guarded API contracts, with no extra retry/authority fields:

- Node local `POST /api/commands/grants/result`: `schema_version:1`,
  `expected_node_id`, `grant_id`.
- Origin node `POST /api/commands/requests/result`: `schema_version:1`,
  `expected_node_id`, `request_id`, `command:{id,grant_id}`.
- EEF owner `POST /api/commands/requests/result`: `schema_version:1`,
  `expected_runtime_id`, `command:{id,grant_id}`.

Existing local-owner/cross-site guards apply. Other origins, changed issuers/scopes
and damaged reports reject. Reads use consistent SQLite snapshots and check stored
size before materializing/parsing report text. Missing evidence never proves no
effects and never authorizes inference, grant renewal or automatic recovery.
