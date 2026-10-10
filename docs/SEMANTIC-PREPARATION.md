# Source-bound semantic method preparation (unreleased after alpha.6)

The running input node can explicitly propose and save a one-step method or an
answerable clarification without authoring JSON inputs:

```text
eefn requests prepare JOB_ID --revision SOURCE_REVISION --operation-id UUID --json
eefn requests plan JOB_ID --json
```

The backend owns read/context/inference/checkpoint/report behavior; the dashboard
is not required. This is **explicit preparation**, not an ambient automatic
trigger, completed general planner or automatic execution. Existing deterministic
`requests compile` commands and explicit inputs remain available.
Caller-supplied `requests submit`/`jobs request-text` sources retain immutable
concrete inputs and use deterministic compilation, not semantic preparation;
see [explicit requests](EXPLICIT-REQUESTS.md).

## Implemented contract

`NodeEngine::prepare_method_candidate` uses the same exact selected
`request_interpreter` role as existing interpretation/clarification resolution.
Missing/ambiguous selection, backend failure and incomplete output reject without
fallback or automatic model selection/download. Its bounded response is a method
candidate or one clarification question, never both:

```json
{"schema_version":1,"method":{"schema_version":1,"template":"system_info","params":{},"constraints":{}},"clarification":null}
```

Unknown fields, duplicate keys, invalid versions, extra authority, oversized output
and malformed questions reject. Only existing one-step templates are candidates;
compound/continuous pipelines are not pretended to be implemented. The task planner
and candidate prompt now share one authoritative template/action/parameter catalogue.
Legacy explicit template behavior and serialized plans remain unchanged.

The method's capability must occur in both the current retained proposal and the
caller-supplied fresh vocabulary. Model routing hints may use user-described areas
or display names, never node/resource/model IDs, roles, backends or compute limits.
Unused parameters and model/device/resource-selection parameters reject. Other
parameter values are untrusted request data, not proof of semantic correctness,
file/device existence, ownership or permissions. EEF compilation must still
apply live provider/model/parameter/resource validation and separate owner approval.

The prompt retains unchanged original text and the complete review/proposal/question/
answer history as separate untrusted messages. The serialized prompt is limited to
64 KiB without truncating answers; output is limited to 16 KiB and inference requests
1024 tokens at temperature zero. Unresolved current questions or continuous/unknown
workloads fail before inference. These are application bounds, not physical model
memory/CPU or OS/backend termination guarantees.

Core results have immutable false execution-authority flags and origin-reported
model attribution. Evidence binds compact job ID, exact positive source revision,
full canonical review digest and exact proposal/answer counts. It cannot attest a
remote model call or model understanding. The candidate itself cannot supply its
attribution, origin, permission or grants. No journal, configuration, grant receipt,
file effect or workload runner is created by the candidate primitive itself.

## Checkpoint workflow

The shared command inspects origin-owned history and the exact operation ID/source
revision before taking inference capacity or requiring a model. A saved plan or
question is inspected without inference, save, provider lookup or authority
renewal, even after later answers/compilation or a withdrawn model role. Conflicting
receipts or an already-retained different plan reject. No saved receipt means a
new explicit inference attempt; this is not an exactly-once model-call claim.

New attempts share interpreter capacity, applied runtime generation and a
20-second inference deadline. Prompts use fresh EEF-disclosed capability names.
Runtime replacement invalidates an in-flight attempt. No second node, fallback
model, automatic download or retry starts.

EEF authenticates origin and correlation receipt, validates fresh capability
scope, immutable saved request context, full review digest/counts and exact
revision. The final journal edit checks the source again under its existing gate.
Fresh self/directionally disclosed executor filtering precedes ranking; EEF owns
final resource/model bindings.

A method stores source evidence in the existing planning record and definition
digest, with `awaiting_approval`, no approval/grant/executor-model load/dispatch. Legacy explicit
records omit the marker and retain original digest bytes. A question appends one
source-pinned proposal changing only clarification fields; original semantics,
earlier proposals/answers and origin remain intact. It can be answered with
`requests answer`, explicitly reinterpreted with `requests resolve`, then prepared
again. Lineage is validated on read/restart; corrupt evidence is not repaired or
replayed automatically.
Older binaries cannot read new source-marker records; preserve backups and do not
downgrade a journal containing prepared plans/questions. Marker-free old records
remain compatible with the new reader and keep their old definition digest.

Guarded local API: `POST /api/commands/requests/prepare`, with exactly
`schema_version:1`, `expected_node_id`, and `command:{id,expected_revision,operation_id}`.
It accepts no caller method, permission, approval or model-binding fields.
Unconfirmed save replies remain uncertain unless known not sent. Runtime changes
after submission may still leave valid EEF checkpoints; inspect or reuse the
exact receipt. No automatic replay or native-effects termination claim.

Automatic open/answer-trigger policy, delegated approval, compound/continuous
methods, semantic quality, complete uncertainty reconciliation and first-time
bootstrap remain unfinished requirements.

Heavier coordinator-assisted planning must use separately approved granted model
work, not a sensitive ordinary-RPC bypass. Default interpreter/bootstrap quality,
delegated owner approval, compound/continuous compilation, physical two-PC and
real-model semantic acceptance remain distinct release gates.
