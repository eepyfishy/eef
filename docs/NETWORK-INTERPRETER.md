# Network interpreter delegation (planned)

Status: raw saving (`4ef0935`) and explicit saved-input interpretation
(`2a49073`) passed all four hosted jobs. Automatic delegation remains planned.
Normal input's raw-first integration passed all four hosted jobs and the
independent source audit at `2b650d6`. It saves before checking for a local interpreter. Saving raw input
does not enable delegation or approve inference; missing models retain input.

The new `eefn requests save-input --text 'Your request'` command uses the running
node to retain raw text in the existing coordinator job journal without a model,
invented proposal, task, approval or execution grant. Use `eefn jobs get ID` to
inspect it and `eefn jobs stop ID` to cancel it. An optional `--operation-id UUID`
allows inspection of the same retained input after a lost reply; changed text
under that receipt is refused. Receipts are scoped to the authenticated origin
and retained history. Deleting history removes that deduplication protection.

The unreleased `eefn requests interpret-saved ID` follow-up passed hosted
verification at `2a49073`. It uses the input node's selected local
interpreter and promotes the same request to review after a durable ownership
checkpoint. An uncertain or already completed attempt is inspected without
another model call. Source identity/revision/digest are read and pinned by the
backend; the user does not need to copy JSON or hashes. Missing models leave the
input saved. Normal `requests input` integrates these stages at the accepted
development checkpoint `2b650d6`. Automatic remote interpretation for
model-free nodes is still unimplemented.

## Intended user behavior

A node with no installed model must be able to accept a user's natural-language
request and use an owner-authorized interpreter on another connected node through
eef. Initial node installation still includes no model. Owners choose models on
the nodes that will serve them; eef itself needs no local model.

The input node owns the interaction. Its identity, area, original text and saved
answers remain the request's origin when another node interprets or executes it.
The dashboard and commands are clients of this backend, not controllers that
must remain open for interpretation or automation to work.

## Existing boundaries to preserve

- [Ordinary RPC](EXECUTION-GRANTS.md) is restricted to deterministic control.
  `llm.infer` is workload execution. Do not add an inference alias under
  `node.configure`, revive legacy chat execution or silently fall back to an
  ungranted model call.
- A model's `request_interpreter` role identifies its purpose, not permission for
  arbitrary origins, unbounded prompts or automatic tool execution. Preserve the
  exact node/backend/model and role binding in execution grants.
- [Current model verification](MODEL-VERIFICATION.md) authorizes one fixed check
  with explicit owner consent. Its allowance must not be reused for user prompts.
- Existing [automation policies](AUTOMATION-APPROVALS.md) approve exact retained
  definitions. A variable-prompt interpreter service needs an explicit bounded
  contract; it cannot pretend new text has an already approved definition digest.
- An interpreter produces an untrusted proposal. Existing proposal validation,
  clarification, deterministic compilation and separate approval of resulting
  tool work remain required. Choosing an interpreter grants no tool permission.

## Durable input before inference

Persist the raw request through the existing job journal before invoking either
a local or a network interpreter. Do not construct a fake semantic proposal or
clarification question merely to satisfy the current review schema.

The pending record needs immutable original text and authenticated request context,
an exact operation receipt, revision and source digest. It has no task, plan,
approval, execution grant or guessed capability. Inspection and cancellation must
work without an available model. It must never enter the legacy execution/resume
paths just because no interpreted draft exists yet.

Interpretation promotes that exact saved input to a reviewed proposal using an
expected revision and digest. Keep the raw source evidence and existing request
identity. Changed text, origin, area, answers or operation reuse cannot silently
replace it. Reusing a receipt after a lost reply inspects retained state; it does
not run inference again. An explicit saved-input interpretation operation must
remain available after an interrupted attempt or later model setup.

Use the existing journal and its count/byte limits. Do not introduce a second
in-memory queue that becomes authoritative after restart.

## Owner authorization and provider selection

The journal-transition increment passed all four hosted jobs at `23d7309`, with
1,630 Windows/headless Rust tests and all 37 native suites. It adds initial
interpretation ownership to the existing `Plan`
record rather than introducing another job queue. The ownership record retains
the original input/context/digest, source revision, explicit attempt ID and input
node runtime. Repeated ownership requests inspect the existing attempt and do
not permit another model call. A coordinator restart marks an unfinished claim
`unknown`; cancellation and inspection remain available under lowered storage
quotas using reserved checkpoint space.

A specific validated transition can promote the same request into ordinary
review, retaining its identity, original text, context and first proposal digest.
The proposal is validated against current disclosed capability names. It creates
no tasks, plan approval or execution grant. Ordinary journal edits cannot claim,
erase or rewrite this ownership evidence. The claimed record reserves 128 KiB
of journal checkpoint space; this is storage accounting, not a model memory limit.

The current follow-up shares versioned checkpoint types across the Rust
workspace and binds explicit local interpretation to the selected node, model,
backend and runtime. The explicit public command and local model-call path passed
hosted verification at `2a49073`. Old records with missing provider metadata stay unknown;
they never become an inferred model selection. This local path cannot claim a
different node as its interpreter or authorize remote inference. Integration into
`requests input` passed at `2b650d6`; remote provider authorization, bounded broker grants and
explicit recovery remain required. A claim itself never authorizes inference.
A completed review still needs separate preparation, owner approval and execution.

Interpretation sharing must be an explicit, configurable service permission.
The unreleased [interpreter policy commands](INTERPRETER-POLICIES.md) now record
the coordinator owner's explicit node scope and bounded limits in the existing
journal. This is policy persistence accepted at `50174aa`; allowance
reservation, fresh provider resolution and automatic delegation remain open.
The coordinator owner controls its allowed origins/providers and aggregate use;
each serving node retains its model/grant policy and the ability to revoke access.
Neither an advertised role, connectivity nor remote-management permission alone
authorizes inference. New policy fields must be optional and disabled when absent.

Resolve only a fresh, independently authenticated, permitted provider with an
active text-capable `request_interpreter` selection. Reuse model metadata,
discovery restrictions, routing and concurrency machinery. Preserve local-only
selection and explicit owner preferences; do not infer a role from a model name,
download a default model or choose a hidden provider outside the origin's scope.

The unreleased [interpreter routing observation](INTERPRETER-ROUTING.md) adds this
read-only selection boundary through the existing registry and private gateway
state. Hosted verification is pending. It pins fresh runtime/session/model and
serving permission metadata, preserves explicit preferences and directional
disclosure, and rechecks before returning. It reserves no allowance or grant and
does not connect normal input to a remote interpreter yet.

Capture the selected node, backend, model, runtime/session and relevant policy
generation before dispatch. A later availability or ranking change does not
substitute another provider inside that attempt. Avoid recursive interpretation:
the serving node executes the exact inference grant, not another delegation call.

## Grant-backed interpretation stages

The complete workflow must cover initial interpretation, clarification answers
and concrete-method preparation. Each stage binds the exact retained source,
revision, prior answers, disclosed capability context and prompt/schema version.
Build or verify its prompt deterministically from those records; never accept an
unrestricted caller-supplied system prompt under the service permission.

The unreleased, data-only `eefn::interpreter_task` contract passed all four hosted
jobs and the independent source audit at `4bbe879`. It reconstructs all three stages through the existing shared
prompt builders and validators, pins the retained source and inference-input
definitions separately, and rejects changed definitions or oversized history
instead of truncating it. It adds no dependency, background service, inference
endpoint or automatic delegation. Existing local token ceilings remain unchanged.
Its own encoded limits do not override smaller enclosing grant/transport budgets.

These digests prove self-consistency, not ownership, permission or which model
ran. A future issuer must independently match the journal and service policy;
the receiver must validate the exact grant, model, session and completion before
accepting output. Validated candidates remain untrusted and cannot authorize
tool execution. This contract alone does not implement the broker below.

### Node-side serving increment (accepted development source)

The next source increment adds `eefn permissions allow interpreter-sharing` and
`eefn permissions deny interpreter-sharing` through the existing feature-wide
permission commands. It is off when absent. Enabling is a saved preference that
needs an explicit node restart to apply; it selects, downloads and runs no model.
Owners still select the serving models and their `request_interpreter` role.

Only the existing accepted-grant driver can execute the new source-bound task.
It requires exact source/context/task-definition, model/backend/role and serving
permission revision bindings. The normal ledger, lease/deadline, stop/policy
watcher and completion checkpoint remain in control. Backend-owned permission
revisions survive ordinary saves/restore and track relevant model-setting changes:
disabling then re-enabling or restoring a removed model role cannot revive old
work. A candidate also needs complete validated provider output and unchanged
current permission at the final response. It remains untrusted and grants no tool
authority. Direct generic execution of this task is refused.

Source `50174aa` passes the
[four-job hosted run](https://github.com/eepyfishy/eef/actions/runs/42552611503)
and independent source/log audit, including node-side execution and actual
permission commands. Synthetic Ollama replies cover the internal admission path;
the additional transport test connects the shipping node client and private TLS
gateway with production fresh-lease admission for all three stages. It checks
complete source-bound replies, accepted/final observation, revocation, duplicate
refusal and retained results across a new connection session.

The issuer, model and observer are fixtures. This does not establish a production
coordinator broker, physical models or automatic routing. Coordinator policy
records exist as described above, but the durable allowance ledger, fresh
provider resolution and three-stage broker remain missing; ordinary input does
not delegate yet.

An owner-approved service policy must make each inference authorization durable,
identifiable and bounded before issuance. Record that it derives from a specific
policy/generation, not a fabricated per-request human approval. Reserve policy
allowances atomically and preserve their use across restart. Reuse the existing
grant issue/consume/accept/complete protocol and exact attempt identity.

Bound request and result bytes, tokens, duration and concurrency per origin,
provider and policy. Keep stop/inspection capacity available under inference load.
Intersect requested bounds with the serving node's applied limits. Shared Ollama
ownership and cancellation uncertainty remain visible; a timeout is not proof a
shared model stopped computing.

Return a source-bound interpretation result through eef to the input node. Validate
complete output and the existing proposal/method schema before updating review.
Malformed, incomplete, oversized or out-of-scope output cannot approve a plan,
create another action, change resource ownership or enable a feature.

The unreleased [receiving-result validation](INTERPRETER-RESULTS.md) increment,
pending hosted verification, regenerates the exact typed candidate from the
returned proposal and granted source as part of grant-report confirmation. It
checks the whole result, including evidence and non-authority flags, rather than
trusting the peer's validation claim. It creates no review promotion or broker
authority by itself.

## Failure and recovery

Disconnect, node/coordinator restart, policy revocation, stale source revisions
and changed provider sessions retire the pending continuation. Preserve the
saved input and any issuance/acceptance/completion evidence. Report known refusal
separately from an uncertain outcome.

Do not automatically replay an uncertain inference, overwrite its receipt,
substitute a new provider or promote partial output. Recover through retained
grant results when available. Any separately permitted subsequent attempt needs
its own durable identity, current source and fresh authorization; never extend
the old grant's authority.

## Implementation and acceptance sequence

1. Add durable raw-input storage and public command/API inspection/cancellation,
   then persist normal input before local inference. Cover failures before any
   model is configured and after input is saved.
2. Add explicit interpreter-sharing policy, provider resolution and bounded
   policy-backed authorization. Saving/inspecting policy starts no model call.
3. Connect all three interpretation stages to the same grant-backed broker and
   existing node workflow, preserving local-only behavior and restart fences.
4. Exercise actual headless commands with an input node advertising no model and
   a separately paired interpreter node. Verify retained origin/area, exact model
   assignment, proposal persistence and absence of downstream tool execution.
5. Verify revocation, wrong origin, changed source/model/session, incomplete model
   output, quotas, lost replies and both-role restart without automatic replay.
   Repeat with real selected models and two physical PCs before claiming that
   end-to-end acceptance. Synthetic model replies remain separately labelled.

These steps are prerequisites for the full delegation behavior, not replacements
for it. Network interpretation does not implement general distributed model
sharding, coordinator replication, continuous media routing or self-upgrades.
