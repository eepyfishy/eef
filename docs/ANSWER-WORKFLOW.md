# Source-bound answer workflow (unreleased after alpha.6)

`eefn requests respond` records an answer to an explicitly selected saved question
and uses the running node's applied policy to resolve and prepare it. CLI and HTTP
are clients of the same backend service. It extends the existing draft, resolver,
preparation/compiler and job journal; no new runner or dashboard controller exists.

```powershell
eefn requests draft JOB_ID --json
eefn requests respond JOB_ID --revision SOURCE_REVISION --text "This node" --operation-id RESPONSE_UUID --json
eefn requests workflow show --json
eefn requests workflow set --resolve-on-answer true --json
eefn restart --wait-seconds 60 --json
```

Use actual retained IDs/revisions and a canonical UUID generated before sending.
Keep the entire response command after uncertainty. The backend never guesses
which request/question a free-text answer refers to, replaces the original input,
changes authenticated origin, or invents a node/resource/model selection.

## Policy

`input_workflow.resolve_on_answer` defaults to **false when absent** in an existing
configuration. Fresh/example configurations enable it. It is independent of
`prepare_on_input`: owners can patch either field, or both, through `workflow set`.
An omitted field retains its saved value; an empty patch is rejected. Saving is
not applying, inferring, restarting, approving or changing existing requests.
Explicitly restart to apply the saved policy. Saved and applied values are shown.

- Resolution disabled: record the exact answer only (`answer_recorded`).
- Resolution enabled: one bounded selected-interpreter call can append a resolved
  proposal or another clarification question. Uncertainty stays a question.
- Resolution and preparation enabled: an eligible resolved one-shot request gets
  one preparation call and compilation. It can still ask a concrete-input question.
  Continuous/unknown requests stay for review, not automatic workload admission.
- A compiled method remains `awaiting_approval`, with pending tasks. Separate
  owner approval and exact execution are always required.

Manual `requests answer`, `resolve` and `prepare` remain explicit primitives.
They do not become automated merely because the workflow policy is enabled.
Installing/selecting a model assigns no interpreter role or approval; exactly one
owner-selected local `request_interpreter` is still required for inference.

## Durable source and receipts

EEF writes an optional `source` checkpoint with every new clarification answer,
including source revision, full pre-answer draft digest, proposal count and answer
count. The answer already retains its exact question, proposal index, text, receipt
and timestamp. Source evidence is computed inside the existing atomic revision
check, not accepted from the caller/model. It is not a signature or execution grant.

Read/restart validation reconstructs only the explicitly declared history prefix
to verify its digest. It never infers a missing revision from timestamps or current
state. Earlier proposals/answers and original text remain intact. The workflow
checks the complete source transition, immutable context and exact save receipt,
then validates resolution/preparation against their complete source lineage.

Older answer records without `source` remain readable and unmodified. Their source
revision/digest is unknown, so their receipts cannot authorize automatic resolution
or preparation. New answer receipt reuse requires the same text and source revision;
conflicts fail unchanged. Keep a backup and do not downgrade a journal containing
new answer-source fields to a build that cannot parse them.

Answer, resolution and preparation use separate logical receipts. Stage receipts
are deterministic, domain-separated canonical UUIDs bound to the node, response
receipt, job and stage source revision. They are not credentials or permission.
The version-1 `node_answer_workflow` report preserves the original response command,
source inspection, answer reply, exact stage commands/replies and last confirmed
result. Source and answer confirmation are separate from aggregate `success` and
`coordinator_acknowledged`; none of these flags means approval or executed work.

Reusing a saved response receipt only inspects the current origin-owned draft,
even if a partial workflow stopped after saving an answer/resolution, the policy
changed, the interpreter disappeared or the coordinator restarted. It never
implicitly resolves, prepares, retries or continues the request. A reused stage
receipt also stops automatic continuation. Completing partial work is an explicit
decision using the existing resolver/preparation commands, not a hidden retry.
Deduplication lasts only while job history is retained, scoped to the original
node/job; deletion/database replacement is not a permanent receipt ledger.

## Bounds and failure behavior

Input and answer workflows share one node-local admission slot. Busy requests fail
immediately without queueing an answer, opening or inference. Existing history,
prompt, token and model time limits apply. The answer workflow has a 180-second
deadline; runtime replacement cancels its local continuation. A possible mutation
remains uncertain, never rolled back or assumed to have stopped an external model.

Source lookup failure, stale revision, wrong origin, no question, a colliding
receipt or exhausted answer history cannot cause a new answer/inference. Later
resolution/preparation failures preserve earlier confirmed checkpoints and exact
receipts. A concurrent explicit answer/edit invalidates a model checkpoint instead
of overwriting the newer history. Confirmed earlier stages and their derived
commands are validated even in failed aggregate replies; `result` is the last
confirmed snapshot, not a promise that no newer checkpoint was committed.

An authenticated checkpoint rejection now retains bounded, origin-private
`preparation.coordinator_rejection` with `preparation.error_code` set to
`preparation_rejected`. The aggregate remains
`answer_preparation_not_confirmed`; rejection alone does not prove no durable
write and leaves uncertainty true. Inspect retained history rather than starting
another answer or inference automatically. See [checkpoint diagnostics](SEMANTIC-PREPARATION.md).

After a lost/corrupted local HTTP reply the CLI reports uncertainty with the entire
pre-send response command. It performs no fallback or replay. Unsupported older
nodes do not fall back to chat, manual answer, raw jobs or execution.

## API adapters

Local-owner-protected `POST /api/commands/requests/respond` uses a strict body:

```json
{
  "schema_version": 1,
  "expected_node_id": "ACTUAL_NODE_ID",
  "command": {
    "id": "ACTUAL_COMPACT_JOB_UUID",
    "expected_revision": 1,
    "text": "This node",
    "operation_id": "ACTUAL_CANONICAL_RESPONSE_UUID"
  }
}
```

Node `/api/chat` accepts an optional strict `response_to: {id, expected_revision}`
alongside `message` and optional canonical `request_id`. When present, `message`
is the answer and `request_id` is its response receipt; the adapter calls the same
service and adds human-readable compatibility `reply`. Without `response_to` it
uses normal input. Supplying a target is not authority to access another origin's
request. Caller origin/permissions/context fields are rejected. Clients should
declare the receipt before sending; no conversational target is inferred.

## Evidence and remaining requirements

The expanded `test-request-input.mjs` fixture uses API-only, independently paired
logical nodes and fake inference. It checks two clarification rounds, latest
questions, exact history/context, saved/applied policy, disabled and resolve-only
paths, current-source races, busy admission, runtime replacement, partial failures,
lost/substituted replies, private origin scope, and read-only receipt reuse after
interpreter withdrawal and coordinator restart. No workload grant is issued.
Rust tests independently check source digests, legacy unknowns, transition integrity,
separate receipts and cancellation after an answer ACK.

This is bounded answer-triggered interpretation/preparation, not general semantic
planning, permanent workflow recovery, automatic workload execution, durable
automation definitions or reusable bounded owner approvals. Friendly pairing/role
setup, default model bootstrap/quality, physical two-PC acceptance, strong
termination, installers/signing and the rest of [the roadmap](ROADMAP-CORE.md)
remain requirements for stable 0.4.0. No alpha.6 artifacts or update feeds changed.
