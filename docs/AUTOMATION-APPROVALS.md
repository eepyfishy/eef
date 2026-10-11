# Bounded reusable approval records (unreleased after alpha.6)

These owner-approval checkpoints extend the existing journal and command
services. Saving a policy never admits a job, consumes a run, issues a grant or
starts execution. The subsequent [explicit occurrence reservation service](AUTOMATION-OCCURRENCES.md)
now consumes bounded authority for independently compiled matching jobs;
`occurrence_admission_available: true` refers only to that owner-command path,
not timers or completed recurring execution. No dashboard controller or second
runner is added. Stable 0.4.0 still requires the remaining acceptance work.

## Owner commands

Use the actual disabled definition ID/digest and current generation, not these
placeholders. The first generation uses `--generation 0`. All bounds are explicit.

```powershell
eef automation-policies approve --definition DEFINITION_UUID --revision 1 --definition-sha256 DEFINITION_HASH --generation 0 --operation-id APPROVAL_UUID --valid-for-ms 10000 --max-runs 3 --max-execution-ms-per-task 1000 --max-result-bytes-per-task 1024 --max-reserved-execution-ms 3000 --max-reserved-result-bytes 3072 --json
eef automation-policies list --definition DEFINITION_UUID --limit 25 --json
eef automation-policies show APPROVAL_UUID --json
eef automation-policies revoke APPROVAL_UUID --revision 1 --operation-id REVOCATION_UUID --json
```

Approval/show replies contain private task parameters, bindings and origin
metadata. List pages omit that scope and user text. Do not publish full replies
in reports/issues. Commands work without browser assets or model inference and
never start a second coordinator. Current separately authenticated grant-capable
executors are required for new approval; historical inspection/reuse is not.

The guarded local-owner endpoint is
`POST /api/commands/automation-policies` with strict
`{schema_version:1,expected_runtime_id,command}`. There is deliberately no node
submission kind or node API/CLI for granting these owner policies. The ordinary
node definition command cannot import an `approve` command. Remote update or
node-management permission is not reusable execution approval.

The owner CLI shares its bounded, no-proxy/no-redirect exchange with definition
commands. It validates the current diagnostic/runtime identity before POST and
the typed reply afterwards. Failed preflight sends no mutation. Lost, substituted
or negative mutation replies remain uncertain even when failure followed commit.
Keep the exact command/receipt; explicitly inspect or reuse it. No fallback,
replacement receipt, automatic retry or implied rollback is supplied.

## Fixed scope, budgets and generations

The policy pins the exact definition ID/revision/digest, immutable source node
and area context, original planning input, operation order/dependencies, parameters,
constraints, execution deadlines and fixed node/resource/model/backend bindings.
It records the current coordinator runtime UUID and authenticated grant issuer
fingerprint. Hashes protect references/integrity, not malicious-file-owner
attestation or Authenticode signing. The existing compiler and native grant
contracts independently validate the retained scope, including after definition
removal releases its larger source snapshot.

New approval rechecks current exact providers/models and their issuer/grant
bounds, then checks the definition/generation again under the existing journal
lock before saving. This is not executor readiness guaranteed at future dispatch;
future occurrences must independently repeat current-provider and permission
checks through the existing grant driver. A saved snapshot never licenses rebinding.

`max_runs` cannot exceed the definition's declaration. Validity is at most the
definition's window and 24 hours, and cannot be shorter than a per-task allowance.
Per-task bounds obey the existing node grant protocol and compiled deadlines.
One full occurrence's worst-case duration/result allowances must fit the aggregate
budgets. Aggregate declared bounds are at most 24 hours of granted duration and
128 MiB of result allowances. These are upper limits, not a promise all declared
runs can fit; each future admission must atomically consume both run count and
worst-case aggregate allowances before approval/issuance.

These declarations are not measured CPU/RAM/power quotas, physical disk allocation,
bandwidth quotas, hard native termination or an already-enforced run ledger.
Node-local grant limits remain necessary and cannot be widened by the policy.
No policy digest can substitute for an existing job's separate approval hash.

Generation is compare-and-swap across retained approvals for one definition.
An explicit new approval advances the last retained generation; concurrent exact
receipts create one record, and a stale generation creates none. The intended
admission fence requires the latest generation, matching current runtime/issuer,
an unremoved definition, unrevoked approval and time inside its window. The pure
`current_at` helper checks only those eligibility facts; it neither consumes a
budget nor proves executor readiness or authorizes an actual grant.

Clock rollback before approval, expiry, runtime/issuer change, supersession,
revocation or definition removal cannot make that record current. Restart retains
history but cannot silently reactivate an old runtime's approval. Reusing a
retained approval receipt is read-only even after expiry/restart/revocation/removal;
it cannot renew times, change scope or recreate a definition. A new runtime needs
new explicit owner approval and generation, not a replay of an old operation.

Revision one is `approved`; explicit revision-bound revocation saves revision two
`revoked`, with its own exact receipt/runtime/observed timestamp. Revocation is
allowed after a backward clock change and never claims remote effects stopped.
The immutable approval digest remains useful for historical reference. Policies
and their generation history are not automatically deleted or compacted.

## Persistence and storage

The additive `automation_authorizations` table uses the existing exclusive
database lock, journal mutex and synchronous checkpoints. Jobs, definitions and
approval records share `jobs.max_count` and `jobs.max_bytes`. Storage diagnostics
keep job `count` and definition `automation_count` semantics and add
`authorization_count`, `authorization_bytes` and `authorization_reserved_bytes`.
New jobs/definitions account for approval costs and vice versa.

Each unrevoked policy reserves 1,024 logical bytes for revocation. Lowered quotas
cannot prevent a size-reducing/reserve-backed revocation or historical read.
Actual disk/SQLite failure can still prevent persistence; there is no physical
free-space promise or hidden pruning. Removed definitions release their snapshot,
but retained policy parameters/history remain private and still consume storage.

Startup validates record identity, schema, fixed compiler/grant scope, definition
references, contiguous generation history and reserved-byte accounting. Corrupt
or future records remain intact and fail closed; no automatic repair, partial
success inventory or activation is performed. Old binaries do not understand
this table/accounting and downgrade compatibility is not claimed.

## Occurrence stage and remaining lifecycle work

The subsequent [reservation stage](AUTOMATION-OCCURRENCES.md) implements the
atomic compiled-job delegation and start/every-issuance policy fences described
below. Timer activation, terminal archiving, broader resource/termination evidence
and the other release requirements remain open. The following invariants also
apply to future automatic triggering:

Each occurrence must be a new independently compiled, origin-bound job. Do not
copy old model evidence into a new job identity, fabricate new interpretation
history, or reuse old approvals, attempts or grants. Compare its normalized
operations/input and origin with the fixed definition scope before consuming
policy authority. Source/definition history must remain unchanged.

Implement one atomic journal transaction for reservation lineage, aggregate
worst-case budget consumption and that new job's delegated approval. Never refund
an uncertain dispatch or create a replacement receipt after uncertainty. Gate
execution start and every issuance on the current policy/definition/generation
inside the existing journal admission gate, not just an earlier HTTP preflight.
Revocation must prevent later unissued work without claiming it stopped a remote
operation already accepted. Actual work must use the existing durable grant
driver, node enforcement, acknowledgement/completion and explicit recovery rules.

Only after that should timers add overlap, missed-trigger/suspend/clock-change,
restart-to-unknown, cancellation and no-replay behavior. Neither a removed record,
missed observation nor an uncertain previous occurrence should trigger replacement
work automatically. Strong native termination, measured full resource budgets,
pipelines, semantic bootstrap, peer/media/cluster behavior, physical two-PC and
installer/Defender/Authenticode acceptance remain required release work.

## Evidence (2026-10-12)

Twelve new tests bring the full workspace/all-target total to 457. Both default
and no-default-feature suites pass, followed by a fresh default executable and
example build. Formatting, diff, JavaScript syntax and the backend/browser source
boundary checks pass separately. The first check caught a Rust type mismatch in
scope hashing; hashing the serialized scope corrected it before these final runs.

Eight journal tests cover immutable source history, exact read-only receipt reuse,
generation compare-and-swap, widening/substitution refusal, independently compiled
scope validation after source removal, retained future-schema refusal, missing
generation history, all-three-record byte accounting and count limits, and
reserve-backed revocation under lowered quotas/backward clock observation. Three
shared contract tests cover bounds, eligibility and strict correlated reports;
one CLI test requires explicit approval/revocation options without executable or
permission-widening commands.

The fresh public CLI/API fixture (`automation-definitions-B1ofi6`) passes using
two separately pinned-TLS logical nodes, without model inference or UI assets. It
covers owner-only policy creation, foreign-node/origin refusals, denial of policy
digest substitution for the source job's approval, sixteen concurrent exact
approval requests saving one generation, pagination without private operations,
revocation, restart history and definition removal without policy renewal or
recreation. Actual owner CLI fault-proxy cases cover lost replies, substituted
scope, negative response after commit, stale runtime and invalid preflight. Exact
inspection/reuse finds one retained record; invalid preflight sends no mutation.
The final journal contains one unexecuted source job, seven disabled/removed
definitions and five policy records. No occurrence, timer or workload is started.

Fresh final-build input/answer (`request-input-luGLfF`) and reviewed
execution/recovery (`request-review-VEATQU`) regressions pass separately. The
earlier intermittent input/busy failures are not diagnosed by these passes.

This is isolated one-PC evidence, not physical two-PC execution, bootstrap model
quality, native termination, measured quotas, scheduling, peer/media/cluster or
installer/signing acceptance. Installed nodes, versioned releases, update feeds
and browser assets are unchanged. Stable 0.4.0 remains open.
