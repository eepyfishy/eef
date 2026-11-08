# Network interpreter delegation (planned)

Status: the first durable raw-input increment is in development and awaiting
hosted verification. The delegation described below remains planned. The existing
ordinary-language input workflow still requires a selected interpreter on its
input node. Saving raw input does not enable delegation or approve inference.
Local interpretation and its existing explicit setup remain supported.

The new `eefn requests save-input --text 'Your request'` command uses the running
node to retain raw text in the existing coordinator job journal without a model,
invented proposal, task, approval or execution grant. Use `eefn jobs get ID` to
inspect it and `eefn jobs stop ID` to cancel it. An optional `--operation-id UUID`
allows inspection of the same retained input after a lost reply; changed text
under that receipt is refused. Receipts are scoped to the authenticated origin
and retained history. Deleting history removes that deduplication protection.

This is not yet integrated into `requests input`, and saved-input interpretation
and promotion are still required. The command currently saves and inspects only;
it cannot resume, interpret or execute its saved input. It is not a completed
model-free interaction workflow.

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

Interpretation sharing must be an explicit, configurable service permission.
The coordinator owner controls its allowed origins/providers and aggregate use;
each serving node retains its model/grant policy and the ability to revoke access.
Neither an advertised role, connectivity nor remote-management permission alone
authorizes inference. New policy fields must be optional and disabled when absent.

Resolve only a fresh, independently authenticated, permitted provider with an
active text-capable `request_interpreter` selection. Reuse model metadata,
discovery restrictions, routing and concurrency machinery. Preserve local-only
selection and explicit owner preferences; do not infer a role from a model name,
download a default model or choose a hidden provider outside the origin's scope.

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
