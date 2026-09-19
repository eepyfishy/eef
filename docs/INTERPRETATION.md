# Untrusted interpretation contract (development)

`eefn::interpretation` is a shared Rust prompt/validation module. This first slice
has unit tests and a test-only CPU evaluation adapter. It is **not yet connected
to node chat, commands, coordinator submissions or workload execution**. No new
user-facing command, interpreter role binding or default model is enabled.

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

Remaining: reviewed model/bootstrap policy, owner-selected interpreter role,
bounded provider adapter and cancellation, node request preview/clarification
commands, authenticated original context, EEF revalidation, and explicit
planning/authorization. The existing deterministic commands continue working
without any model and are unchanged by this module.
