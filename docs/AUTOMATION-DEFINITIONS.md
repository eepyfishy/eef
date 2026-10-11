# Durable automation definitions (unreleased after alpha.6)

This increment implements source-bound authoring and retention, not automatic
execution. It extends the existing `JobStore` database, lock and storage accounting.
There is no second job runner, queue, dashboard controller, implicit owner approval,
timer worker or executable fallback.

## Node commands

Use the running node that owns the source request. Explicit one-step requests can
be compiled without an LM; natural-language sources still use the existing reviewed
interpretation/preparation flow. Inspect the source before defining automation:

```powershell
eefn requests submit --text "Inspect node status" --template system_info --json
eefn requests plan JOB_ID --json
eefn automations define --operation-id DEFINITION_UUID --from-job JOB_ID --revision SOURCE_REVISION --plan-sha256 SOURCE_HASH --name "Regular status" --every-ms 1000 --max-runs 3 --valid-for-ms 10000 --json
eefn automations list --limit 25 --json
eefn automations show DEFINITION_UUID --json
eefn automations remove DEFINITION_UUID --revision 1 --operation-id REMOVAL_UUID --json
```

Replace the placeholders with actual retained IDs, revisions and hashes. `JOB_ID`
is the compact lowercase job UUID; definition/removal IDs are canonical UUIDs.
Retain the creation ID before sending it. If delivery is uncertain, inspect that
definition ID; do not generate a different ID merely to retry. Ordinary successful
authoring is not approval or execution of the source or any future occurrence.

`list` supports a bounded lexical `--after DEFINITION_UUID` cursor. Pages include
only summaries owned by this authenticated node; they exclude source text, task
parameters and private paths. `show` returns the private exact retained snapshot.
Both work independently of a browser and currently available model/provider.
The local coordinator-owner API can inspect/author definitions across origins,
using an exact runtime ID; that is not delegated node authority.

## Definition and lifecycle

Creation requires an exact current `awaiting_approval` compiled one-shot source,
with matching revision and plan hash, no owner approval and no execution record.
The compiler's existing retained-state validator independently checks it. Unknown,
continuous, cancelled, approved, executing or stale sources cannot be promoted by
this command. A source's immutable node/area/request context is retained, not
reconstructed from the creator's current location or a model suggestion.

The definition records the exact source snapshot, its canonical digest, the full
creation command and a definition digest binding these to identity/context/time.
Hashing is integrity/reference checking, not code signing or remote attestation.
No approval, grant, executor assignment, results or operation receipts are copied
as reusable authority. Source history is not changed by authoring.

The version-one lifecycle currently has only:

- Revision 1, `disabled`: immutable source snapshot retained; no timer or work.
- Revision 2, `removed`: snapshot payload released, compact creation/removal receipts
  retained; still no timer or work. Removal is not a remote stop operation.

Identical retained creation receipts reuse the original definition without
reinterpreting, refreshing bindings or inspecting a changed source. Reuse after
removal returns the removed record and cannot recreate it. Conflicting receipt
contents fail. Removal is revision-bound and identical receipt reuse is explicit.
Another node cannot inspect, remove or reuse a guessed ID to take ownership.

The timing fields declare a future activation window, not an already-running
schedule: `every_ms` is 1,000–86,400,000; `max_runs` is 1–128; `valid_for_ms` is
1,000–86,400,000 and at least one interval. These are bounded authoring fields,
not enforced runtime/resource quotas. No clock starts and no missed triggers
are replayed by saving a definition. There is deliberately no enable/approve/run
command yet; unsupported fields or lifecycle states are rejected.

## Persistence, storage and replies

`automation_definitions` is an additive table inside the existing job database,
under the same exclusive coordinator lock and synchronous checkpoints. Source
read/validation and insertion share its existing journal mutex; no definition
commits independently of the checked source revision. Existing jobs are neither
rewritten into automations nor granted new authority during migration.

Job and definition counts share `jobs.max_count`; their UTF-8 JSON payloads and
existing job recovery reserves share `jobs.max_bytes`. `jobs` storage reports add
`automation_count` and `automation_bytes`, while `count` remains the job count.
Creating either kind accounts for both kinds. Lowering a quota does not block
read-only inspection or size-reducing definition removal. Removal receipts still
consume history count and compact payload space; there is no automatic pruning
or permanent deduplication guarantee after database replacement/deletion.
Logical quotas are not promises of free physical disk space or RAM/CPU budgets.

Startup validates retained definitions and frozen sources without activating
them. Damaged/future records are retained and cause a clear failure, not repaired,
ignored or returned as a successful partial inventory. Older executables do not
understand this additive table/accounting: downgrade compatibility is not claimed.

The shared typed node command uses the existing authenticated submission path.
Schema/node/receipt mismatches fail before sending. Confirmed rejection, not-sent
and unknown delivery remain distinct. Lost, malformed or substituted replies do
not license hidden retries. Reports explicitly expose `timers_started: false`,
`execution_authorized: false`, `dispatched: false`, and `automatically_retried: false`.
Listing/showing removed or disabled definitions is not readiness for automation.

## Remaining execution requirements

Bounded reusable owner approval against the exact definition, issuer identity,
generation/revision, expiry and aggregate run/resource budget remains required.
Each occurrence needs atomic consumption and its own durable job/attempt/issuance
lineage before dispatch through the existing reviewed grant driver. Revalidate
current providers, resources and node-local permissions; saved snapshots cannot
license stale bindings or changed parameters.

Implement and test overlap, missed-trigger, suspension/clock change, cancellation,
restart-to-unknown, explicit recovery and no-replay behavior together. Do not turn
a removed definition, old approval, missed observation or uncertain completed
operation into a new occurrence automatically. Pipeline authorizations, general
planning, physical resource/termination evidence and second-PC acceptance remain
separate required work. Stable 0.4.0 and finished recurring automation are not claimed.

## Evidence (2026-10-12)

Thirteen new Rust tests cover strict timing/receipt/reply contracts, ownership,
source revision/hash checks, unchanged source history, removed-receipt reuse,
compiler validation even after record hashes are recomputed, shared count/byte
quotas, lowered-quota compaction, restart retention and damaged/future records.
Both workspace/all-target feature-mode suites pass all 441 tests after formatting;
a fresh default executable/example build passes separately.

The final `node tools/test-automation-definitions.mjs` fixture
(`automation-definitions-wHI93Z`) uses actual CLI source submission and authoring,
two separately authenticated pinned-TLS logical nodes, foreign-node refusals,
pagination, a deliberately lost save reply, 16 concurrent exact creation calls,
verified coordinator restart and removed-receipt reuse. Exactly one concurrent
creation is new; the source stays unchanged awaiting approval and no occurrence
or execution appears. This is one-PC evidence, not physical second-PC acceptance.

The original shared test fixture used a noncanonical job ID and failed four
new contract tests. It was corrected to an actual compact UUID; production ID,
scope, lifecycle and timing validation were not weakened. The first public
fixture (`automation-definitions-wZ0EEC`) passes before the additional CLI-source
and concurrency checks; it is not substituted for the final stronger fixture.
