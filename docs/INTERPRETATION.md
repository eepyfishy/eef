# Untrusted interpretation contract (v0.4.0a4)

`eefn::interpretation` is a shared Rust prompt/validation module. Alpha.4
adds `eefn requests preview --text TEXT`, backed by this
module and an explicitly owner-selected `request_interpreter` model role. It is
**not connected to chat auto-interpretation or workload execution**. No default
model or automatic selection is enabled. The unreleased increment after alpha.6
adds explicit `eefn requests review` for stateless coordinator review, described
below; older published preview behavior remains unchanged.

## Proposal, not authority

The caller supplies the original input and the current capability-name context.
The model supplies only this strict JSON proposal shape:

```json
{
  "schema_version": 1,
  "intent": "information",
  "goal": "Inspect node status",
  "context_hints": ["this node"],
  "constraints": [],
  "suggested_capabilities": ["system.info"],
  "complexity": "simple",
  "workload": "one_shot",
  "needs_clarification": false,
  "clarification": null
}
```

All fields are required, including nullable `clarification`. Unknown and duplicate
fields, unsupported versions, invalid types/enums, fenced/trailing/multiple JSON
objects and truncated output are rejected rather than repaired. There is no
fallback that turns a failed parse into an executable request.

- `intent`: `information`, `action`, `conversation`, `unknown`.
- `complexity`: `simple`, `moderate`, `complex`, `unknown`. This is a semantic
  hint, never measured CPU/RAM cost or authority to choose a bigger model.
- `workload`: `one_shot`, `continuous`, `unknown`.
- Unknown intent/workload requires `needs_clarification: true` and a nonempty
  question. A non-null question and the flag must agree.

Origin identity, target IDs, resource bindings, executable command fields,
permissions and grants are absent from the proposal schema. Capability hints
must be unique and belong to the caller's supplied list. That list is context,
not an execution grant. Text hints remain untrusted even after schema validation;
they may contain wrong or malicious suggestions and must never be run as code.

The wrapper preserves original text exactly and always returns
`execution_authorized: false`, `model_output_is_untrusted: true`, and either
`review_required` or `clarification_required`. It never dispatches, saves or
downloads anything. Valid syntax does not establish correct interpretation,
detect every ambiguous/consequential request or prove prompt-injection resistance.
Those policy decisions cannot be delegated to `needs_clarification` alone.

## Bounds

| Item | Limit |
| --- | --- |
| Original input | 32 KiB UTF-8 bytes |
| Raw model proposal | 16 KiB before JSON parsing |
| Goal | 2,048 bytes |
| Context hints / constraints | 8 each, 256 bytes per string |
| Suggested capabilities | 8, distinct and present in runtime context |
| Supplied capability context | 64 distinct names, 64 ASCII identifier bytes each |
| Clarification question | 1,024 bytes |

Empty/whitespace-only text and control characters other than tab/newline/CR are
rejected. Bounds apply to bytes, not character counts. The future model adapter
must separately bound HTTP bodies, output tokens, elapsed inference time and
concurrency **before** collecting output for this validator.

The existing general inference adapters now cap backend JSON responses at 4 MiB
(including chunked bodies) and expose reported finish metadata; see
[response semantics](NODE_MODELS.md#inference-response-bounds-development).
This is separate from the tighter 16 KiB proposal limit and does not connect the
interpreter or impose its future inference/concurrency budget.

## Evaluation and remaining integration

Build the test-only `interpretation_fixture` example and run the CPU harness with
`--profile contract`. It uses this module's prompt and validation code, not a
second JavaScript implementation. Neither example executable is packaged.
The earlier `baseline`/`detailed` benchmark profiles intentionally use a smaller
three-field classifier schema and are not directly equivalent accuracy tests.

The preview command now reuses existing Ollama/llama.cpp inference with a
20-second service deadline, 512 output tokens and one concurrent preview per
running node process. Exactly one applied model must support `llm.infer` and
carry the `request_interpreter` role. Missing/ambiguous roles never silently pick
another model, start a runtime or download anything. Partial/unknown completions
and invalid proposals fail closed. The weak runtime binding is invalidated on
restart, cancelling the local wait and rejecting stale results. It does not
promise that an external Ollama service stops all computation on disconnect.
Ordinary inference traffic is not governed by this preview-only semaphore.

The original text goes only to the configured model backend (which may be an
owner-configured remote Ollama endpoint), not to EEF. Preview has no job journal
entry, execution grant or persistent receipt. A lost reply does not automatically
retry. Closing the CLI may leave bounded inference running until service timeout;
it cannot cause an action to be executed. No UI is required, and a paused or
unconfigured coordinator connection does not prevent preview.

The unreleased review command sends original text and a typed proposal through
the existing bounded node submission mailbox, with fresh reply-routing identity
separate from its request UUID. The gateway replaces origin and capability
context with accepted registration snapshots for the authenticated session.
EEF revalidates all proposal bounds and semantic consistency with the shared
validator, without invoking its brain, planner or job store. The response retains
original text and server-stamped context. The node service and CLI reject
mismatched identity/text or execution flags. Capability advertisements remain
untrusted hints, never authority. The review is ephemeral and creates no durable
receipt, job, grant or implicit clarification answer. Losing its reply never
causes replay. Preview still does not send text to EEF.

The subsequent unreleased `requests open/open-proposal` commands explicitly save
a review-only record in the same durable job journal. Origin-owned `draft`,
`answer` and `revise-proposal` commands retain original text, questions, answers,
proposal history and revision/receipt checks across restart. They do not run tasks
or grant approval. Answers are facts for a future resolver, not an implicit model
re-interpretation; even a revised confident proposal still requires review.
Stateless preview/review semantics above remain unchanged.
See [durable review commands](COMMANDS.md#durable-request-review-and-clarification-unreleased-after-alpha6).

Remaining: reviewed model/bootstrap policy, better candidate/prompt quality,
automatic clarification resolution, owner approval, and explicit planning/authorization.
Automatic input interpretation is still absent. The existing deterministic
commands continue working without any model. See [command usage](COMMANDS.md#request-preview-development).
