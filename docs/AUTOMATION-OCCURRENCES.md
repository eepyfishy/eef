# Explicit bounded occurrence reservation (unreleased after alpha.6)

This connects reusable owner approval to the existing reviewed grant driver.
It is manual command/API admission of an independently compiled matching job,
not timers, a copied inference history, a second runner or an automatic trigger.
Saving a reservation approves that job but does not start execution. Stable
0.4.0 and completed recurring automation are not claimed.

## Commands and authority

Create a fresh request through the running origin node's ordinary explicit or
reviewed compilation commands. Its input, normalized operations, parameters,
dependencies, timeouts and node/resource/model/backend bindings must exactly
match the approved definition. Its request identity must be fresh; origin node
and area must match. Original source jobs cannot become occurrences. No model
proof, preparation history, task identity, approval, attempt or grant is copied
from the source into a new job.

Use the actual policy ID/digest and fresh job revision/plan digest:

```powershell
eef automation-occurrences reserve --policy POLICY_UUID --policy-sha256 POLICY_HASH --job FRESH_JOB_ID --revision JOB_REVISION --plan-sha256 JOB_HASH --operation-id RESERVATION_UUID --json
eef automation-occurrences show RESERVATION_UUID --json
eef automation-occurrences list --policy POLICY_UUID --limit 25 --json
```

Only the guarded coordinator-owner endpoint
`POST /api/commands/automation-occurrences` accepts this strict command, wrapped
in `{schema_version:1,expected_runtime_id,command}`. Nodes cannot submit owner
reservations or import an approval through their request/definition commands.
An exact retained receipt is inspection, even after expiry, revocation, runtime
replacement or source-definition removal. It cannot renew times or dispatch.

Show/reserve replies include a private frozen checkpoint of the **actual newly
approved job**. Do not publish these replies. List pages omit this snapshot,
parameters, original text and model evidence. Lost, malformed/substituted or
negative mutation replies retain uncertainty, even when rejection followed
commit. Preserve and inspect/reuse the original command/receipt; no automatic
retry, replacement receipt, fallback or implicit rollback is supplied.

## Atomic journal admission

Current exact providers/models, authenticated issuer and node-local grant
bounds are checked before new reservation. Under the existing engine admission
gate and journal mutex, the core rechecks the exact job, policy digest, latest
definition generation, unremoved definition, unrevoked policy, current runtime
and issuer, and time inside the approval window. It also rejects overlapping or
uncertain previous occurrences of the same definition, including earlier policy
generations. A successful completed job or explicit cancellation with no issued
attempt permits another occurrence. Failed, interrupted, abandoned, missing or
partially issued/cancelled work does not become permission to replace it.

Each accepted reservation consumes one run and a full occurrence's worst-case
per-task duration/result allowances. The append-only reservation ledger derives
counts and aggregate consumption from all retained receipts; cancellation,
failure, non-dispatch, revocation and restart refund nothing. The remaining time
window must cover the full reserved duration, without guaranteeing provider
latency, native termination or future capacity. A new explicit policy generation
has its own owner-declared budget but cannot bypass unresolved previous work.

One SQLite transaction saves the new job's delegated approval and immutable
reservation lineage together. Failure of either write, validation or quota
checks rolls both back. The production clock is sampled under the lock, with a
final expiry/clock check before commit. There is no independent counter commit,
new job construction from old inference evidence, or grant dispatch in this
transaction.

Jobs, definitions, policies and occurrence records share the existing logical
count/byte limits. Storage reports add `occurrence_count` and `occurrence_bytes`,
without changing the other counts' meanings. Actual disk failure remains possible;
these are not physical disk, CPU/RAM, bandwidth or power reservations. The frozen
approval snapshot consumes real journal bytes and is independently compiler/
grant validated, not inferred from a cached source hash alone.

## Execution and lifecycle fences

After inspecting the newly approved job, explicitly use the ordinary node
`requests execute` path with its revision and delegated approval digest. It uses
the existing scheduler reservations, durable grant issuance, node enforcement,
acknowledgement/completion records and explicit result recovery. No ordinary
owner-approval command can reinterpret a delegated receipt as a direct approval.

Execution start and **every grant issuance** check current policy authority
under the same journal lock as their checkpoint, inside the engine admission
gate. Revoked, expired, superseded, removed-definition or old-runtime authority
prevents new unissued work; saved policy snapshots cannot rebind a provider or
widen permissions. A revocation after a durable issuance does not claim that
grant was never sent, remotely cancelled or physically stopped. Node-local
enforcement and bounded leases still apply. Unknown dispatch/completion never
licenses replay, remaining-step resumption, migration or replacement automatically.

Restart retains reservations and their budget consumption but cannot reactivate
the old runtime's delegated authority. Historical inspection is not readiness.
Live job authority must match its retained reservation; missing, future or
damaged records/generation/ordinal history fail closed without repair or pruning.

Reserved jobs cannot use ordinary `jobs remove`: their evidence is needed for
overlap/replacement decisions. The subsequent explicit archive path below moves
proven terminal evidence out of live jobs without erasing it or resetting budgets.
Unresolved partial work still cannot be archived or replaced.

## Explicit terminal archiving (subsequent unreleased increment)

After an actual completed reviewed job, or cancellation with **no issued attempt**,
inspect its exact final revision and the original reservation ID/digest. Then:

```powershell
eef automation-occurrences archive --reservation RESERVATION_UUID --reservation-sha256 RESERVATION_HASH --job JOB_ID --revision TERMINAL_REVISION --operation-id ARCHIVE_UUID --json
eef automation-occurrences archive-show ARCHIVE_UUID --json
```

These are owner commands on the same guarded endpoint, not node permission or
remote-update commands. No provider, model, new approval or current unrevoked
policy is needed to archive historical evidence. The current runtime envelope
still must match. An active driver, stale revision, wrong reservation/hash/job,
failed, interrupted, abandoned or partially issued/cancelled occurrence refuses
archiving. A status label alone is not proof: normal compiler/approval/grant,
acknowledgement/completion/recovery and task-result validation still applies.

One transaction retains the **actual final job snapshot** and immutable archive
receipt, removes the live job, and revalidates the entire occurrence history.
It rolls back both writes on insertion/deletion/validation/quota failure or a
backward commit clock. The terminal snapshot is never a resumable job. The
original reservation, ordinal, policy generation and consumed run/duration/result
allowances remain unchanged. Subsequent overlap checks use the validated archived
proof. Missing, future, damaged or unexpectedly reintroduced live history fails
closed; it cannot imply a refund or permission to replace unknown work.

Reuse the **same** archive command/receipt after a lost, substituted or negative
post-commit response; `archive-show` inspects it without mutation. Reuse remains
read-only across restart, expiry and lowered quotas. A tombstone rejects opening
a new job using the archived original node/request identity, even if the old
input is repeated. Archive receipts and reservation receipts are distinct.

Archives share logical storage/count quotas. One archive replaces one live job
in the count, but does **not** free its evidence bytes; metadata may require
additional quota headroom. Storage exposes `occurrence_archive_count` and
`occurrence_archive_bytes` separately and includes them in available/over-limit
accounting. An archive above **2 MiB** is refused before commit; the job remains.
This bounded full-evidence stage is not payload compaction, private-data erasure,
physical disk reclamation or automatic retention. Archive/show replies remain
private (original input, parameters, results and native receipts); list pages
still exclude those snapshots. No browser controller or automatic cleanup is added.

## Remaining work

Add timers only after trigger/overlap/restart-to-unknown, missed-trigger, suspend,
clock-change, cancellation and no-replay behavior are validated together. There
is currently no automatic occurrence-job creation, semantic evidence reuse,
trigger catch-up, timer activation or model download. General/heavy planning,
reusable pipeline grants, bootstrap quality, measured full resource enforcement,
strong native termination, peer/media/cluster behavior, physical two-PC and
installer/Defender/Authenticode release acceptance remain in scope.

## Reservation checkpoint evidence (2026-10-12)

The preceding reservation increment added twelve tests and was verified with
469 passing tests in each workspace/all-target feature mode: nine journal tests,
two shared contract tests and one owner CLI test. Its fresh default executable/
example build, formatting, JavaScript syntax, diff and backend/browser source
checks passed separately.

Journal tests cover an actual new compiled job's approval and source preservation,
exact historical receipt reuse, no overlap or cancellation refund, exhausted
run/duration/result budgets, policy time/runtime/issuer/revocation/generation
fences, conflicting receipts, insert-fault transaction rollback, retained restart
history/future-row refusal, a second-write byte-quota failure and expiry/backward
clock change before commit. The clock tests prove neither approval nor budget
consumption survives a failed final-time check. Shared tests cover strict command/
delegation options and bounded private-scope-free correlated lists; CLI tests
reject missing authority references and executable/refund/renewal flags.

The fresh public fixture (`automation-occurrences-4vLRpb`) uses two separately
pinned-TLS logical nodes, no browser controller and no models. Sixteen concurrent
owner reservation requests save one occurrence. The original source stays
unchanged; original/foreign-origin jobs and overlap are refused. Explicit ordinary
node execution completes a reserved native `system.info` grant through the existing
driver. Revocation and coordinator restart refuse later unissued work while the
original time window is still open; current-runtime owner queries confirm the
policy-fence rejection reason, not an envelope or provider failure. Fault-proxy
owner CLI calls cover dropped, substituted and negative-after-commit replies;
exact inspection/reuse finds one reservation and never starts work automatically.

The first public run (`automation-occurrences-2iNrvd`) failed a fixture assertion:
it expected `null` for optional absent plan fields, which the existing serializer
omits. It is not counted as passing. Assertions now require field absence; no
production behavior, authority bound or retry policy was changed for this correction.

Fresh final-build regressions also pass existing definitions/owner policies
(`automation-definitions-RgZ8SC`), input/answers (`request-input-tG6LvM`) and the
broader reviewed execution/recovery workflow (`request-review-UoGRUU`). The last
uses isolated fake inference; it does not establish real interpreter quality.
The earlier intermittent input/busy failures remain separate unproven issues.

These are local logical-node/native-system-information checks, not physical
two-PC, real interpreter quality, general pipeline, hard native termination,
measured full quotas, timer lifecycle or installer/signing acceptance. Installed
nodes, published versions/installers, feeds and browser assets are unchanged.
Stable 0.4.0 remains open.

## Terminal archive evidence (2026-10-12)

Eight additional tests bring both passing workspace/all-target feature modes
to **477** (EEF library 186, owner CLI 11, installer 6, EEFN library 268,
node CLI 6). Five journal tests cover terminal archival across expiry/restart,
permanent budget consumption and request tombstones, exact receipt conflicts,
insertion/deletion/commit-clock rollback, stale references and nonterminal
refusal, byte-quota rollback/shared count replacement, and missing/future/
reintroduced history refusal. Two shared contract tests check bounded strict
private evidence and correlated non-dispatch replies; the CLI test requires
all removal references and rejects execution/refund/retry/force options.

The fresh default executable/example build, formatting, diff, script syntax
and backend/browser source guard pass. Current-build definitions/policies
(`automation-definitions-Al9E8e`), input/answers (`request-input-sOCMdx`) and
the broader reviewed execution/recovery fixture (`request-review-NPiW9N`)
pass separately. The last uses fake inference, not real semantic quality.
The earlier intermittent input/busy failures remain unproven.

The final public fixture (`automation-occurrences-8i85cy`) uses local pinned-TLS
logical nodes, not physical two-PC acceptance. It retains actual native
system-information completion/receipts, archives exactly
once across 16 concurrent owner calls, inspects unchanged proof after restart,
archives unissued cancellation even after policy revocation/runtime replacement,
refuses original-request replay, nonterminal removal and foreign-origin calls,
and preserves uncertainty for dropped/substituted/negative post-commit archive
responses. Exact command reuse finds one archive and never refunds or reactivates
authority. The preceding archive fixture (`automation-occurrences-npmUJZ`) also
passed before the last two public refusal assertions were added.

These checks do not establish physical two-PC, hard native termination,
measured full quotas, timer activation or
installer/Defender/Authenticode acceptance. No installed node, version/feed,
browser asset, model or release artifact changed. Stable 0.4.0 remains open.
