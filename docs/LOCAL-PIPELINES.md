# Finite local pipelines

Unreleased after alpha.6. The command/compiler integration exposes finite local
dataflow through the existing reviewed job/grant system. Node startup advertises
`pipeline.run`; ordinary RPC/generic execute still cannot run it. Local child
permissions, exact parent authority and the original deadline remain required.
This is not completed continuous/media support or stable-release acceptance.
No second job system, grant bypass, service, listener or workload runner is added.

## Submit, review and execute

Use a schema-1 `PlanInput` file containing `template:"local_pipeline"`,
`params:{pipeline:DEFINITION}` and normal node-selection constraints:

```text
eefn --json requests submit --text "Process the selected input locally" --title "Local dataflow" --file pipeline.json
```

Submission saves immutable concrete input and compiles one durable task, normally
`awaiting_approval`; it does not interpret, infer, create a grant or start stages.
Inspect it with the existing `requests plan`/EEF owner inspection commands, then
use the existing separate exact-digest approval and execution commands. Owner
review sees the complete graph and union of stage capability hints. No JSON,
model result or browser can insert approval/execution flags into submission.

The coordinator filters disclosed/current candidate nodes against **every**
declared stage, then uses existing locality/constraint/load ranking among suitable
executors. A local graph has one executor: resources must belong to that node,
models/backends/roles must exactly match current eligible registration, and all
stage capabilities/providers must be current. Resource-backed providers require
explicit stage resource IDs; no implicit identity substitution or model fallback.
Root model/resource constraints are rejected instead of silently ignored.

The same checks run again before approval and grant issuance. The retained graph,
context, binding and digest cannot be rewritten by recalculating a digest alone.
Stage data references are deferred inputs, not fabricated placeholder values in
planning; static parameters still receive provider-schema checks. Runtime resolves
and validates actual data under the parent budget. This template is explicit
command authoring, not silently added to LM semantic-method suggestions.

Issuance atomically reserves the root and every distinct local stage provider
under the existing registry lock. Global and exact-node ceilings count one parent
future; provider ceilings count each used registration once, including repeated
stages using that registration. A busy/missing/changed child declines the whole
reservation without incrementing any provider. Exact stage resource choices are
checked again; late release cannot decrement replacement registrations. All
providers remain conservatively reserved for the parent lifetime. These counters
are coordinator-local future admission, not physical quotas or remote stop proof.

## Definition and dataflow

`eefn::pipeline::FinitePipeline` has schema version 1, 1-16 ordered steps,
a maximum encoded definition of 64 KiB and declared retained data of 1 KiB-1 MiB.
Step IDs are unique bounded ASCII identifiers. Each step has a fixed capability,
action, literal parameters and optional exact model/backend/role or resource.
Unknown definition/step/input fields fail rather than being interpreted as
permissions, routing or runtime instructions.

Operation shape validation reuses the existing grant contract for supported
one-shot operations. It is a validator, not authority: it contains no issuer,
session, nonce, lease or journal claim. Actual grant validation still checks all
existing identity, context, duration/result, exact executor and consumption rules.
The execution core has an exact parent pipeline grant shape containing
one definition, no substituted top-level model/resource bindings and retained
data no larger than the parent/owner result budget. Compound execution must
receive that exact reviewed parent grant; a definition alone never permits work.
Nested pipelines, management/update operations and unsupported plugin contracts
are not structurally accepted as steps.

Data inputs name an earlier step, an RFC 6901 result pointer and a fixed target
parameter. Forward references, cycles, duplicate IDs/targets, literal conflicts,
malformed pointers and missing/type-invalid data are rejected. Image wrapping is
explicit and does not flatten arrays or reinterpret object data as text.
Inputs may supply prompt/images, file content, HTTP body, audio bytes or output
text. They cannot change capability/action, model/backend/role, physical resource,
file path, program/arguments or URL. This keeps operation identities fixed; it
does not imply that arbitrary sink contents are harmless or approved for use.

Static identities are normalized into adapter parameters only when consistent
with their dedicated fields. With a known executor, resource IDs must belong to
that node. Structural validation without an executor is not proof of availability,
ownership approval, local permissions or safe routing. Model literals have strict
prompt/messages/image types and existing generation bounds.

`resolve_step` validates the whole graph and applies its declared encoded data
limit. A bounded counting writer checks selected data before cloning it; no
unbounded serialized copy is allocated merely to find a budget violation.
Reference counts identify when a future runner may release an earlier result.
The private execution core accounts for retained encoded outputs and drops a
result after its last input reference is consumed. Resolved input is separately
bounded before adapter invocation. These are encoded-data bounds, not physical
RAM/VRAM/CPU or whole-backend transient-allocation guarantees.
An isolated `PipelineStep::resolve` helper uses the global ceiling; it is neither
proof of prior execution nor a substitute for parent graph validation.

## Grant-owned finite execution core (subsequent increment)

The existing `AcceptedGrant::run` future owns all sequential stages under its
original monotonic deadline, current-policy watch and one node work admission.
No extra worker, child grant, nonce, lease extension or replay path is created.
Each stage uses an operation-shape view, not a fabricated standalone authority.
Admission checks all stage capabilities/permissions and model readiness before
the existing root claim. The filesystem preflight now distinguishes read from
write, and explicit physical resources must be present/available for the exact
operation. Policy changes to a later stage can stop pending current work.

During execution, permission/selection checks remain active for the whole graph.
Completed model stages do not require their process to remain resident afterward;
the next actual model stage checks readiness again. Actual inference still uses
the existing selected backend and model lifetime pins. Data resolution cannot
change those identities. Progress is bounded runtime-local metadata on the exact
current `last_workload.grant_id`; it contains no intermediate payload and is not
durable stage evidence or a stop acknowledgement.

Only the final output and ordered completed stage IDs become successful result
data. A known unsuccessful native exit retains its exit/output under failed-step
evidence and prevents subsequent stages. Unknown execution/data/budget errors,
deadline, dropped future or permission loss preserve the existing parent unknown
state and consumed nonce; no stages resume or migrate. The completion validator
rejects substituted/missing stage identities and false pipeline success. A client
future drop still does not prove shared backend/native effects have stopped.

## Required remaining integration and acceptance

Continue on the existing durable job/task/attempt and grant machinery:

- Verify and expand compilation/review of the complete source-bound graph, with all fixed executor,
  resource, model and dataflow choices visible. Keep plain commands model-free.
- Verify issuance of the entire graph under one exact reviewed authority; structural child
  projections must never become fabricated standalone grants or new nonce claims.
- Expose and validate finite stages locally under the original monotonic deadline/work admission.
  Recheck current permissions and actual resources/model availability at safe
  stage boundaries and while work is pending; data cannot widen operation scope.
- Verify retained/temporary input and result budgets, release final-use
  references, report bounded progress/output and distinguish failed/unknown work.
- Persist existing acceptance/final evidence and preserve no-unsafe-replay after
  cancellation, disconnect or either-role restart. Do not claim strong stop from
  dropping a future; add actual supported acknowledgements/termination boundaries.
- Align all literal parameter types/bounds with actual adapters before exposure.
  The prototype rejects click `interval`, which the current bundled adapter
  ignores, rather than presenting that setting as active.
- Keep public headless command/one-PC acceptance, then add physical multi-node/media
  acceptance. Pure schema/dataflow tests do not demonstrate camera or audio use.
- Integrate and accept reusable automation authorization for frozen finite graphs.
  Its reconstructed review must retain the complete deterministic child capability
  union, not just the parent operation. Disabled definitions alone do not prove
  pipeline policy approval, fresh trigger admission or safe recurring execution.

Continuous loops, triggers, detection/VAD/STT/event pipelines, backpressure,
offline buffering/privacy retention, peer transport, media relays/presence,
physical resource budgets, measured routing/migration and coordinator fencing
remain in the full 0.4.0 roadmap. This finite foundation does not replace them.

## Structural verification checkpoint (2026-10-13)

Final workspace/all-target suites pass **581 tests** in each feature mode:
EEF library 220, owner CLI 16, installer 6, EEFN library 328, node CLI 11.
Fresh default workspace binaries/examples and formatting/diff/backend-browser
source checks pass. Nine pipeline cases cover static identity/data resolution,
resource ownership, exact temporal references, substitutions/unknown fields,
fixed operation identities, missing/wrong-type data, pointer escaping,
encoded budgets, literal/resolved model bounds and supported data sinks.
The additional grant case proves this definition cannot authorize `pipeline.run`
under the current one-operation contract or ordinary RPC boundary.

An initial test-only borrow error was fixed by cloning a duplicate input before
mutating its vector. Two initial negative tests were strengthened to reach the
intended temporal/data-binding guards rather than pass on unrelated invalid
operations. A failed atomic source patch made no changes and was reapplied in
file order against the current content. No authority, expiry or policy was relaxed.

Before the final standalone-literal/model-resolution guards, intermediate both-mode
suites also passed 581 tests; public native lifetime `process-lifetime-71E6XN`,
model metadata `model-metadata-gzCiiU` and owned model policy `model-policy-gTLJUf`
passed together with chain exit 0. These demonstrate the unchanged leaf grant
paths, not final pipeline execution, native/memory guarantees or physical media.
No installer, release/feed, installed runtime, physical node, UI or model changes
are made by this source foundation. The compiler/runtime/parent-grant and full
roadmap integration gates above remain required.

Fresh final-binary leaf regressions pass native process lifetime
`process-lifetime-D0GLVU`, model metadata/selection `model-metadata-xELIUu` and
owned policy/on-demand `model-policy-S6oFsD`, with full chain exit 0. These remain
existing-operation acceptance, not a public pipeline execution claim.

## Private execution-core checkpoint (2026-10-13)

Final workspace/all-target suites pass **592 tests** in both feature modes:
EEF library 220, owner CLI 16, installer 6, EEFN library 339, node CLI 11.
Fresh default workspace binaries/examples, formatting/diff and the backend-browser
source guard pass. Production startup still does not opt into this runtime.
The tests privately enable it under the existing parent `AcceptedGrant` machinery;
they do not bypass its common identity, lease, permission, claim or final journal.

New cases prove one consumed parent/no replay, admission/ACK without stage effects,
specific child permission refusal before consumption, local file-model-file dataflow
through a synthetic HTTP backend with the exact selected model, final-use buffer
release versus aggregate retention refusal, missing-data/budget-to-unknown behavior,
known native exit 17 without later writes or false success, the original deadline
ending a waiting model stage, and permission loss for a later stage interrupting
current work before lease expiry. Generic execute/ordinary RPC cannot invoke the
compound core. Completion shape rejects substituted/missing stage IDs and false
success. These are isolated execution-core tests, not public reviewed pipeline,
actual interpreter/model quality, physical media/resource or multi-PC acceptance.

The initial child-permission test exposed an existing filesystem preflight gap:
read and write shared capability presence without checking the requested action.
Admission now checks that action before claim/effects. Model adapter aliases are
also supplied from the licensed fixed stage identity, using the same normalization
as a single model grant; resolved data cannot choose a different backend/model.
No grant permission, expiry, replay, identity or nonce rule was relaxed.

Final-binary public leaf regressions pass native lifetime `process-lifetime-Cu0YOK`,
model metadata/selection `model-metadata-w6vyDt`, owned policy/on-demand
`model-policy-jsMNN5` and origin input/answers `request-input-9iEGE4`, with full
chain exit 0. Earlier `process-lifetime-UIcn2f` terminated with native code
`0xc0000409` during startup/setup, with no normal harness exit/failure report;
no fixture processes remained and the queried recent Application events did not
establish a cause. It is not passing evidence. The isolated rerun with uncaught
exception tracing passes, but that does not prove a fix or explain the native exit.

No installed runtime/node, release/feed, installer, UI asset, real shared service
or model changed. Public compilation/commands/admission, physical network/media,
resource budgets, acknowledged stop/restart and all other full 0.4.0 gates remain
required; the private availability switch is not a user-configured bypass.

## Public compiler and admission checkpoint (2026-10-13)

Final workspace/all-target suites pass **601 tests** in both feature modes:
EEF library 227, owner CLI 16, installer 6, EEFN library 341, node CLI 11.
Fresh default binaries/examples, formatting/diff, JS syntax and the backend-browser
source guard pass. New compiler tests require the whole disclosed current graph,
exact model/backend/role/resource identities, deferred data versus literal types,
and unchanged source/digest bindings. Explicit graph capability unions can exceed
eight only under their immutable concrete-input contract; ordinary untrusted
model proposals retain their existing eight-hint limit. Durable history checks
the exact deterministic union, not just the parent capability.

Initial public `local-pipelines-8w2gYm` failed before job/grant creation because
durable review validation still used the one-step template whitelist and one
capability hint. The retained read-only database check found zero jobs; this is
not passing acceptance. The corrected review boundary reuses the common concrete
capability union without relaxing model proposal validation. Subsequent public
`local-pipelines-VJJnUV` and fresh-binary `local-pipelines-z00YHQ` passed submission,
separate approval/execution, local file-model-file dataflow, one parent/no replay,
later permission loss and exact retained history across restart. These preceded
the subsequent child-provider admission fix, not final evidence for that fix.

Review then identified parent-only capacity accounting. Compound issuance now
atomically reserves every distinct child registration for the parent lifetime.
Four new cases cover concurrent child admission, one-parent global/node counts,
deduplicated stages, failed-last-child/resource scope without partial mutation,
and retired/replaced registrations without decrementing successor counters.
Provider counters need not sum to the parent count; no physical quota or remote
stop follows from a reservation release.

Before the capacity fix, the fresh-binary regression chain passed native lifetime
`process-lifetime-6dViBe`, model metadata `model-metadata-4yRqpD`, model policy
`model-policy-RKRJLu`, input workflow `request-input-HdtyMU` and reviewed requests
`request-review-Qq9tvH`. Earlier unexplained native exits remain unproven; these
passing fixtures do not retroactively explain or fix them.

Final-binary public `local-pipelines-Q4rJxx` passes the compound admission
assertions and known unsuccessful native-stage path. The live read-only observer
holds creation-verified root/child handles and observes both exit; exit 17 keeps
`completed`/`success:false`, `pipeline_stage_unsuccessful`, failed-step output and
a failed coordinator job. Later writes never run, exact execution receipt reuse
cannot replace native identities, and failed/completed histories survive EEF
restart unchanged. This is finite local native evidence, not a production stop
acknowledgement or proof of unrelated external effects.

Preceding `local-pipelines-Lz5uiQ` failed during native graph submission before
that job/grant existed (two earlier jobs remained completed/interrupted). The
fixture's local-ready wait did not establish fresh EEF registration after node
restart. Its barrier now requires both registered nodes and the actual current
executor-native provider before making a new submission. The fresh isolated run
passes; the earlier failure is not acceptance and no uncertain mutation was
automatically replayed. No production permission or admission rule was relaxed.

Final-source regression chain passes scheduling `scheduling-U6lgk3`, native
lifetime `process-lifetime-9BQ7GU`, model metadata `model-metadata-5uO96e`, model
policy `model-policy-moidew`, input workflow `request-input-oIeQfC` and reviewed
execution/recovery `request-review-JjkmPg`, with full chain exit 0. Synthetic
inference remains synthetic; these passes do not explain earlier native exits.

This integration changes development startup advertisement, not installed nodes,
published artifacts/feed, installers, UI assets, real services or model files.
Synthetic inference and two logical nodes on one PC do not demonstrate real
interpreter/model quality or physical two-PC/media/resource acceptance. Full
continuous, peer, stop, bootstrap, cluster and stable-release gates remain open.
