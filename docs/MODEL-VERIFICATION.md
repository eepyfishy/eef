# Explicit model verification (unreleased)

This locally accepted development increment is not a completed automatic
setup-verification feature. Hosted acceptance is separate. Setup inspection
continues to be read-only and must not claim inference readiness from selection,
activation or a prepared test request.

## Fixed input, existing authority

The new shared definition binds schema, actual node, applied runtime, owner
operation UUID and exact backend/model. The node requires exactly one applied
`request_interpreter` matching that selection. It authors a fixed `generate_text`
request for that same node/backend/model/role: a short greeting, temperature zero
and 32 output tokens. Callers cannot replace the prompt, attach tools/files or
widen the concrete input under the verification marker.

Source, approval and execution receipt domains are separate custom UUIDs.
Runtime changes do not silently allocate a new source under the same owner
operation: the changed definition conflicts with retained original intent.
Receipts are correlation, not authentication or grants. Existing job-history
retention remains the deduplication boundary.

The running node's ordinary authenticated submission path stamps origin/area
and uses the existing durable review/compiler. There is no invented origin,
ephemeral node, direct model RPC, new execution engine or automatic approval.
Preparing input does not infer, load/download a model, restart or change settings.
Existing coordinator-owner approval and normal execution grants remain required.

## Preparation backend

The node backend exposes local-owner POST
`/api/commands/setup/model-verification/prepare` with a 4 KiB body bound. Its
definition contains `schema_version`, `node_id`, `node_runtime_id`,
`operation_id` and `model:{backend,model_id}`. The authenticated remote action
`prepare_model_verification` additionally requires current local management
consent. It uses ordinary work capacity, not a reserved read-only status lane.
No source preparation can enable that consent itself.

One owned preparation slot refuses competing calls without queue/retry. Runtime
changes cancel its local continuation; a source may already be saved and its
outcome remains unknown. No rollback, inference success or physical stop is
inferred. A confirmed reply contains the existing concrete-request report and
its normal approval/execution state, not new authority.

## Explicit coordinator-owner check

The owner backend accepts POST `/api/commands/node/model-verification` with a
4 KiB body limit and the existing owner/origin protection. A start pins the
coordinator runtime, node, owner operation UUID and node-authored definition,
and requires `approve_inference:true`. It prepares input through the actual
node, validates the exact retained source/task/binding, calls normal owner
approval with 30-second/4-KiB grant limits, and calls normal durable issuance.
It cannot supply arbitrary prompts, delegate approval, or change permissions.
The node runtime fence applies to source preparation; ordinary execution grants
separately bind the actual authenticated executor session and current provider.

The typed CLI is a thin client:

```
eef node verify-model --node NODE_ID start --allow-inference
eef node verify-model --node NODE_ID inspect --operation-id OPERATION_UUID
```

Start also accepts `--operation-id` to retain a chosen UUID before sending. The
client sends `start_selected` with explicit inference consent. eef checks retained
history first, then resolves a fresh operation using confirmed read-only setup
and permission snapshots. It requires exactly one saved/applied interpreter and
local management consent, pinning the permission read to the observed node
runtime. No client-side setup controller, first-candidate fallback or remote
self-grant is used. The explicit-definition `start` API remains available and
keeps its original intent-conflict behavior and report shape.

A fresh blocked `start_selected` returns `not_ready` and a typed
`readiness_issue` for missing/ambiguous interpreter, saved/applied mismatch,
unavailable runtime/receipt transport, or missing local management consent.
Confirmed shared setup guidance takes precedence when the interpreter is absent
or unapplied: pending setup approval, no approved default and retained/unavailable
setup evidence are not treated as instructions to replace the interpreter.
Unconfirmed setup/permission reads stay `unconfirmed`, never imply consent and
never fall back or retry. No source, approval or execution is requested in those
preflight outcomes. Fixed backend-owned guidance is shared with the CLI; arbitrary
peer errors are not copied into it. The extension is emitted only for the new
command, so existing explicit-definition start/inspect replies remain compatible.

A lost reply returns that exact owner/source receipt
with uncertainty, never a client retry. Backend inspection needs neither a live
node nor its current model selection. Starting an already retained exact intent
only inspects it, even if it is merely awaiting approval; it never resumes an
uncertain earlier workflow into authority. Retained selected starts also work
after model/runtime changes and while the node is disconnected; they inspect
the original definition without first reading its current setup or permissions.
Existing job-history retention is the deduplication boundary,
not a permanent promise after owner removal of history.

`returned_text` requires a bounded nonempty result confirming the exact retained
grant and executor session. Definitive final reports already held by the eef
journal are read locally, even with the node disconnected or no model selected;
`evidence_source` distinguishes coordinator-journal and node-journal evidence.
Unknown local outcomes may be inspected against newer node evidence without
reconciliation; if that read is unavailable, retained unknown stays unknown.
If no verified final report is retained locally, a disconnected node cannot
provide missing output; inspection stays unknown/unconfirmed, never successful.
Running, no text, failed, unknown, missing and
unconfirmed are distinct states. Reports contain correlation and mechanical
evidence, not private model prose. They never approve model quality, current
health, an interpreter default or automatic onboarding.

## Acceptance status and remaining scope

The backend-selected start/readiness follow-up is locally accepted, unreleased.
Both full Rust feature modes pass 1,104 tests plus one existing ignored profiler
each across fourteen groups. Fresh headless apps/helpers and all thirty native
suites pass sequentially. All 302 evaluated source/resource/workflow and nine
native artifact hashes match; no matching native fixture processes remain
observed. The read-only record is
`.validation/model-check-readiness-acceptance.json`.

Synthetic native `model-verification-milbhQ` additionally checks that the CLI
does not read setup itself, confirmed missing/pending interpreter and management
consent block without creating a source, unconfirmed/disconnected setup does not
infer, and retained CLI starts work offline and after selection/runtime changes.
Real CPU `real-model-crr4WA` passes the selected owner command through ordinary
grants, one-attempt history/no replay, model removal, lifecycle and on-demand
checks with unchanged catalog-verified cached weights and full-regression apps.
The independent record is
`.validation/model-check-readiness-real-cpu-acceptance.json`; no matching fixture
app/model processes remain observed. No weights were downloaded or copied.
Published `1babe4f` also passes its four-job hosted run and all twenty-eight
configured native suites in exact order; the subsequent Windows shell follow-up
remains separate. Automatic inference
authority, approved defaults/interpretation quality, first-run health, physical
two-PC and stable-release gates remain separate requirements. The counts in the
historical checkpoint below do not cover subsequent edits.
Nine initial focused tests passed. The first full run was deliberately stopped
during compilation after review found missing-interpreter guidance could
contradict an existing setup/approval attempt. Its partial logs and source hashes
are retained under `.validation/model-check-readiness-before-guidance-*`, not
accepted for the corrected source. This was not an observation timeout or a
test failure. The first corrected test build exposed a test-only use of a private
policy constructor; the fixture now uses the existing public `Saved::new` API
without widening runtime visibility. That compiler failure is retained in
`.validation/model-check-readiness-guidance-focused.log`. All ten corrected
coordinator tests pass, including preserved setup/approval/default guidance and
unavailable setup evidence. Corrected full/native and real-CPU acceptance is
recorded above, not inferred from the earlier partial/failed builds.

- Local synthetic native, malformed/lost-reply, no-replay, real CPU and full
  regression acceptance pass. The focused native fixture `model-verification-lhIyiw`
  passes source-only authoring, explicit owner workflow, lost reply/receipt
  inspection without replay, empty output, revoked consent and both restarts.
  Four node fence tests and the CLI consent test pass. Before the offline-history
  correction, the full headless mode passed 1,098 tests plus one existing ignored
  profiler. The default-mode compile was deliberately stopped after code review
  found inspection unnecessarily depended on a live node despite having a final
  coordinator report. Partial evidence is preserved under
  `.validation/model-verification-before-offline-fix-*`; it is not acceptance of
  the corrected source. The corrected five coordinator tests and native
  `model-verification-IHLHUN` pass, including disconnected history across eef
  restart and offline retained start without node contact or execution.
  Both corrected full feature modes pass 1,099 tests plus one existing ignored
  profiler each across fourteen groups; all 302 source/resource/workflow hashes
  match. Fresh headless apps/helpers build successfully; all thirty native
  suites pass in one sequential run. The read-only record is
  `.validation/model-verification-acceptance.json`: 302 source/resource/workflow
  and nine native artifact hashes match, and no matching fixture processes remain
  observed. Rebuilt apps and the CPU runtime match the separately accepted real
  CPU check's hashes. Published source `a6752ad9648ea6412c0daf6e57fa28cfbe970f42`
  also passes [hosted run 40124073155](https://github.com/eepyfishy/eef/actions/runs/40124073155):
  all four matrix jobs and all twenty-eight configured native suites in exact
  order, independently audited in `.validation/a6752ad-ci-acceptance.json`.
  This does not accept the subsequent backend-selected/readiness follow-up.
  The first native
  run failed because its inventory assertion used
  raw registration fields instead of the normalized inventory contract; this
  failure is retained, not accepted or retried unchanged.
- Automatic setup-to-inference verification needs a separately defined opt-in
  and authority scope. This preparation path does not add delegated approval,
  automatic inference, an approved model default or complete first-run readiness.

## Corrected-source real CPU follow-up (2026-10-25)

Isolated fixture `real-model-gMurOM` passes the explicit owner command through
normal review/grants against the catalog-verified cached Qwen3.5-2B artifact.
The test selects the interpreter role and explicitly saves disabled thinking
only in its private fixture. One attempt produces retained text; exact start
reuse does not replay it, and evidence remains inspectable after the model is
removed from the applied selection. The existing unload/reload, restart,
retained-result, idle-unload and on-demand checks pass as well.

The read-only record is `.validation/model-verification-real-cpu-acceptance.json`:
302 source/resource/workflow hashes, both app hashes and the trusted CPU runtime
match; the cached artifact remains unchanged, no weights were downloaded/copied,
and no matching fixture app/model process remains observed. This does not claim
semantic interpretation quality, a project default, current persistent health,
automatic onboarding, physical two-PC, quota/physical memory measurement, signing,
Defender or stable-release acceptance. Both Rust feature modes and the combined
native regression pass; hosted acceptance remains separate.

`tools/test-model-verification.mjs` exercises authoring, the ordinary separate
owner approval/grant path, and the typed owner workflow against a synthetic
provider. It is not real model quality, physical two-PC or stable-release evidence.
