# Source-bound semantic method preparation (unreleased after alpha.6)

This is a working core candidate/schema/prompt/inference primitive, **not yet a
user command or automatic compiler**. Existing `requests compile` still accepts
explicit concrete inputs; no new HTTP endpoint, saved job transition or ambient
trigger is enabled by this increment. The remaining integration below is required
before claiming users can prepare a concrete plan without authoring its inputs.

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
file/device existence, ownership or permissions. Future EEF compilation must still
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
file effect or workload runner is created by this primitive.

## Required integration, still pending

1. A shared node command must inspect the origin-owned retained source and any
   exact saved receipt before inference, then obtain fresh EEF-disclosed context.
2. It must share single-capacity/runtime-generation guards and bounded cancellation
   with existing node interpretation. The core primitive alone does not fetch that
   context or provide service-level capacity/restart invalidation.
3. EEF must verify authenticated origin, full source digest/revision/counts and
   unchanged request context atomically while compiling against fresh allowed
   executors. It—not model parameters—owns bindings and the final concrete plan.
4. Retain evidence and compilation in the existing job journal/definition digest;
   exact retries inspect rather than infer/save again. Never silently overwrite a
   concurrent answer/proposal, rebind an approved plan or hide an uncertain reply.
5. A new question must be durably source-pinned before becoming an answerable
   clarification; returning a question from the primitive is not that checkpoint.
6. Expose CLI/API only after the complete read/context/infer/checkpoint/report path
   is tested without a dashboard, including lost replies and runtime restart.

Heavier coordinator-assisted planning must use separately approved granted model
work, not a sensitive ordinary-RPC bypass. Default interpreter/bootstrap quality,
delegated owner approval, compound/continuous compilation, physical two-PC and
real-model semantic acceptance remain distinct release gates.
