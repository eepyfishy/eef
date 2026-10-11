# Runtime-owned input workflow (unreleased after alpha.6)

The running node owns a bounded input workflow over the existing interpreter,
durable review journal and preparation/compiler services. CLI and HTTP adapters
do not control its decisions. No new job system, browser controller, automatic
owner approval or execution path is introduced.

```powershell
eefn requests workflow show --json
eefn requests workflow set --prepare-on-input true --json
eefn restart --wait-seconds 60 --json
eefn requests input --text "Inspect this node's status" --operation-id INPUT_UUID --json
```

Use an actual canonical UUID for `INPUT_UUID`. Preserve it before submission;
after an uncertain reply do not generate a replacement just to retry. `--ask`
uses the same service but generates its receipt. It never starts another node,
pairs it, reconnects a paused connection, downloads/selects models or changes
configuration. Missing node/interpreter/coordinator is an explicit failure.

## Policy and states

Fresh/example configurations set `input_workflow.prepare_on_input: true`.
Older/custom configurations missing this field stay review-only (`false`).
The subsequent [answer workflow](ANSWER-WORKFLOW.md) adds independently configurable
`resolve_on_answer`, enabled for fresh configurations and false when absent in
existing ones. `workflow set` can patch either field without replacing the other.
The owner can save either value through the workflow command. It reports saved
and applied policy independently; saving performs no inference or restart and
does not change existing requests. Restart explicitly to apply it. Interpreter
selection remains owner-controlled: exactly one selected local model must have
the `request_interpreter` role. Installing a model alone is insufficient.

For a new receipt the node first asks EEF for origin-scoped retained history.
An unavailable, malformed, truncated or ambiguous lookup cannot license a new
opening. A confirmed empty lookup permits one interpretation and durable save.

- `clarification_required`: interpretation requires an answer, or preparation
  saved a source-bound concrete-input question. No answer is guessed.
- `review_only`: preparation is disabled, or the retained proposal is not an
  eligible one-shot request. Continuous/unknown work is not auto-compiled.
- `awaiting_approval`: one preparation call proposed a valid method and EEF
  confirmed its compilation using current disclosed resources/providers. Tasks
  remain pending. Separate owner approval and exact execution are still required.
- `historical_snapshot`: the input receipt already belongs to a retained request.
  The current private draft is inspected without interpretation/preparation, even
  after restart, policy change, permission/model withdrawal or a partial workflow.
- `preparation_not_confirmed`: the source save was confirmed but the requested
  preparation was not. The report preserves the exact preparation command and
  available nested error/evidence. This is not a successful preparation or proof
  of rollback. Inspect before deciding what to do.

Identical input receipt reuse requires the original text and authenticated node
origin. Conflicting text fails unchanged. Receipts are retained only while their
job history exists; history deletion/database replacement is not a permanent
deduplication ledger. Another independently authenticated node cannot inspect
the source by reusing its UUID. Lookup alone never authorizes work.

## Evidence, limits and failure behavior

The version-1 `node_input_workflow` report retains original opening, optional
historical inspection, exact preparation command, optional preparation reply and
the actual result. `source_confirmed` is separate from workflow `success`.
`coordinator_acknowledged` confirms the requested input workflow, not approval,
execution or completed work. No workload result is fabricated.

The preparation receipt is a deterministic, domain-separated UUID bound to node,
input receipt and retained job ID. It is not a secret, signature or authority.
Its source revision comes from the confirmed opening. Confirmation checks the
full saved source digest/lineage as well as text, immutable context, job/revision,
runtime, candidate and receipts; a schema-valid substituted source is rejected.

Only one input workflow per node runs at a time; a competing input fails busy
without queueing it. Existing interpreter capacity and per-stage prompt/reply
limits still apply. The workflow has a 135-second deadline. Runtime replacement
cancels its local continuation; after a possible save the outcome remains
uncertain, not a rollback or native/backend termination guarantee. No operation
is automatically replayed, resumed or reconciled. Reusing a partial input receipt
only inspects it; finishing preparation requires a separate explicit command.

The node API adds strict, local-owner-protected POST routes:

- `/api/commands/requests/input`: existing preview body
  `{schema_version:1, expected_node_id, request_id, text}`.
- `/api/commands/requests/workflow`: `{schema_version:1, expected_node_id,
  command:{operation:"show"}}` or `command:{operation:"set", prepare_on_input:BOOL}`.
  `set` also accepts optional `resolve_on_answer`; at least one field is required.

Node `/api/chat` calls the same input service and adds compatibility `reply` text.
It uses the latest saved question, distinguishes saved-source preparation failure
from unconfirmed opening, and identifies historical inspection without claiming
new work. The full receipt remains present. CLI rejects unconfirmed/corrupted
workflow replies while preserving the pre-send input UUID and uncertainty; older
nodes return unsupported without falling back to chat/open/jobs or execution.
Manual preview/review/open/prepare commands retain their explicit behavior.

## Acceptance and remaining scope

`node tools/test-request-input.mjs` runs API-only, separately paired logical nodes
with fake inference. It checks enabled/disabled saved/applied policy, exact source
context, questions and continuous deferral, partial preparation, busy rejection,
runtime replacement, lost/corrupted/substituted-source replies, role withdrawal,
origin isolation and receipt reuse across coordinator restart. No workload grant
is issued. The first-run/browser fixture also checks fresh-policy browser/CLI
preparation, pending approval and persisted plans without dispatch.

These checks do not prove real-model interpreter quality, physical two-PC
acceptance, strong termination, installer security or complete non-coder setup.
Bounded answer-triggered resolution/preparation is covered by the subsequent
[answer workflow](ANSWER-WORKFLOW.md). Source-bound durable automation,
reusable bounded approvals, lightweight model bootstrap and friendly pinned
pairing/role setup remain required on the [full roadmap](ROADMAP-CORE.md).
Published alpha.6 artifacts and stable-only update feeds are not replaced here.
