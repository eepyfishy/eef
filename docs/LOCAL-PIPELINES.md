# Local pipeline development contract

Unreleased after alpha.6. This is the structural foundation for roadmap stage 7
and later camera/audio workflows, **not an executable pipeline feature**.
Current `ExecutionGrant` still licenses one operation. `pipeline.run` is not
advertised, not accepted as a grant and not enabled by a dashboard or CLI alias.
No second job system, grant bypass, service, listener or workload runner is added.

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
Compound execution must eventually receive its own exact reviewed parent grant.
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
There is currently no running buffer manager or physical RAM/VRAM/CPU guarantee.
An isolated `PipelineStep::resolve` helper uses the global ceiling; it is neither
proof of prior execution nor a substitute for parent graph validation.

## Required integration before exposure

Continue on the existing durable job/task/attempt and grant machinery:

- Compile and review the complete source-bound graph, with all fixed executor,
  resource, model and dataflow choices visible. Keep plain commands model-free.
- License the entire graph under one exact reviewed authority; structural child
  projections must never become fabricated standalone grants or new nonce claims.
- Run finite stages locally under the original monotonic deadline/work admission.
  Recheck current permissions and actual resources/model availability at safe
  stage boundaries and while work is pending; data cannot widen operation scope.
- Account for retained/temporary input and result budgets, release final-use
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
