# Local pipeline development contract

Unreleased after alpha.6. This is the structural/execution-core foundation for
roadmap stage 7 and later camera/audio workflows, **not an exposed pipeline
feature**. Production startup does not enable or advertise `pipeline.run`, and
production node admission rejects it. A config, model output, plugin name or
dashboard cannot turn that availability switch on. Compiler/public command and
versioned acceptance integration remain required. No second job system, grant
bypass, service, listener or workload runner is added.

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
The execution-core increment has an exact parent pipeline grant shape containing
one definition, no substituted top-level model/resource bindings and retained
data no larger than the parent/owner result budget. Production runtime support
remains off until review/compiler integration is ready. Compound execution must
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

## Required integration before exposure

Continue on the existing durable job/task/attempt and grant machinery:

- Compile and review the complete source-bound graph, with all fixed executor,
  resource, model and dataflow choices visible. Keep plain commands model-free.
- Compile and issue the entire graph under one exact reviewed authority; structural child
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
- Add public headless command/one-PC acceptance, then physical multi-node/media
  acceptance. Pure schema/dataflow tests do not demonstrate camera or audio use.

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
