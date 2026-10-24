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
client checks a confirmed read-only setup snapshot and requires saved/applied
interpreter agreement. A lost reply returns that exact owner/source receipt
with uncertainty, never a client retry. Backend inspection needs neither a live
node nor its current model selection. Starting an already retained exact intent
only inspects it, even if it is merely awaiting approval; it never resumes an
uncertain earlier workflow into authority. Use inspect after a node runtime or
selection changes. Existing job-history retention is the deduplication boundary,
not a permanent promise after owner removal of history.

`returned_text` requires a bounded nonempty result confirming the exact retained
grant and executor session. Definitive final reports already held by the EEF
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
  `model-verification-IHLHUN` pass, including disconnected history across EEF
  restart and offline retained start without node contact or execution.
  Both corrected full feature modes pass 1,099 tests plus one existing ignored
  profiler each across fourteen groups; all 302 source/resource/workflow hashes
  match. Fresh headless apps/helpers build successfully; all thirty native
  suites pass in one sequential run. The read-only record is
  `.validation/model-verification-acceptance.json`: 302 source/resource/workflow
  and nine native artifact hashes match, and no matching fixture processes remain
  observed. Rebuilt apps and the CPU runtime match the separately accepted real
  CPU check's hashes. Hosted acceptance remains separate and pending.
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
