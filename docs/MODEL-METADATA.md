# Model registration metadata (unreleased)

This extends the current registration protocol and existing model registry; it
does not create a second scheduler, change framing/authentication, or grant model
execution. Published v0.4.0a2 installers do not contain these changes.

Each `models[]` entry retains `model_id`, `backend` and optional legacy `modality`.
New nodes may add the bounded `model_metadata` object:

```json
{
  "model_id": "owner-selected-model",
  "backend": "ollama",
  "modality": "vlm",
  "model_metadata": {
    "schema_version": 1,
    "capabilities": ["llm.infer", "vlm.analyze"],
    "input_modalities": ["text", "image"],
    "output_modalities": ["text"],
    "roles": null,
    "lifecycle": null,
    "availability": null,
    "resource_estimates": {"ram_mb": null, "vram_mb": null, "size_bytes": null}
  }
}
```

The three capability/modality lists and schema version are required when the
object is present. Each list (including optional roles) has at most 32 unique
identifiers of 1-64 ASCII letters, digits, dots, underscores or hyphens. Empty
lists are explicit: they do not trigger fallback to the legacy label. Fields
outside this metadata schema, unsupported versions, invalid types and duplicate
identities are rejected before accepting registration. The existing maximum is
64 models per node. Identity is the tuple `(node_id, backend, model_id)`.

Lifecycle values are `unloaded`, `loading`, `ready`, `active`, `warm_idle`, `error`;
availability is `available` or `unavailable`. Omitted/null optional values are
unknown. Resource estimates are optional unsigned integers, not measurements or
proof of capacity. Roles are descriptive identifiers, not authority or configured
role bindings. Paths, credentials and arbitrary nested metadata are not projected
into peer discovery or the model inventory command.

## Compatibility and current behavior

### Busy placement hints (under verification)

With an applied node-local model request ceiling, the next development increment
adds optional registration field `model_request_admission`. Its schema-one
limit, occupied/free exchange counts and explicit false queue/provider-execution
flags use the same bounded observation as owner policy inspection. The node's
model metadata, capabilities, permissions and registration receipt stay unchanged.
This separation matters: availability and capabilities also participate in
checks of already admitted interpreter continuations. A full gate must not
invalidate the exchange that filled it.

Acquisition and release notify one coalesced watch owned by the current node
connection. Registration refreshes in place using the same authenticated session
and receipt context, without restarting or replaying work. This also covers
Ollama, whose model snapshot otherwise has no owned-process lifecycle watch.
The current partial incoming frame is retained across refreshes. No detached
publication loop, provider probe or waiting request queue is added.
The gateway derives whether registration is a same-session refresh; it overrides
any peer-supplied classification. Such refreshes update observers without
re-emitting `node.connected` connection automation events.

The gateway validates the optional hint before accepting registration; the model
registry validates and replaces models and their admission observation together.
Unsupported versions/fields, inconsistent counts or invented provider execution
are rejected without replacing the prior model snapshot. Missing legacy hints
mean unknown extra capacity, not a retained full gate or proof of free resources.
Uncapped registrations omit the field. Capacity-change refreshes require the
authenticated coordinator's `model_admission_updates` schema-one offer; missing
or unsupported offers cause no extra capacity refreshes. Older coordinators can
ignore the optional initial hint while the executing node still enforces its
local ceiling. The hint is not projected into
peer discovery as additional authority or private assistant context.

New route selection skips a reported full node without rewriting lifecycle,
model availability, roles or capabilities. Only changed admission observations
are published after the initial subscribed refresh. If no
eligible model remains, route inspection returns no candidates; this does not
authorize another node, fallback, migration or retry. Hints can become stale at
once, so the executing node's atomic admission still decides whether a new
exchange may start. Admitted source leases and historical result access remain
independent. General resource placement and measured allocation remain open.

Hosted verification is pending. New command checks hold Ollama and owned llama.cpp
requests, require route withdrawal while the workload remains active, inspect
controls/results and check restoration without reconnect. A private interpreter
exchange test also requires its active continuation to remain authorized and
complete while its capacity advertisement excludes new placement. Completion, stop,
restart, lost replies and owned-process exit retain their original uncertainty
and no-replay checks. Synthetic fixtures do not establish physical acceptance.

A subsequent read-only command increment is also pending hosted verification:
`eef models route` reports `placement_observation`, distinguishing no matching
model from matching connected models excluded by full gates. Counts use the
same model/role/backend filters and one registry snapshot; several models on
one full node do not become several full nodes. Candidate counts and display
limits retain their existing meaning. Missing hints remain unknown. This
observation never reserves capacity or changes scheduling/continuation authority;
see [command details](COMMANDS.md#added-in-v040a3-model-route-inspection).

### Existing registration behavior

- No `model_metadata`: normalize explicit `text`/`vlm` labels using the previous
  compatibility rules. Unknown/missing labels provide no capability hints.
- Present `model_metadata`: it is authoritative for capability filtering, even
  when the legacy label disagrees. Invalid metadata is never silently downgraded.
- New Ollama and local-GGUF advertisements emit this object alongside the legacy
  fields, based on the existing selected modality/projector configuration. They
  do not infer capabilities from model names or claim measured resource telemetry.
  Development GGUF nodes additionally report observed group startup/process-exit
  state; see [lifetime monitoring and its limits](NODE_MODELS.md#model-startup-development-after-v040a3).
  Old coordinators ignore the additional object and continue using legacy fields.
- `eef models list` exposes normalized metadata with `metadata_source` equal to
  `model_metadata_v1` or `legacy_registration`, and separate registration freshness.
- The registry validates and atomically replaces a node's complete model snapshot,
  including empty snapshots, without removing other nodes' models or load metrics.
- Existing text/vision routing requires the corresponding advertised capability
  and text input/output (plus image input for vision);
  reported `unavailable`, `loading` or `error` models are excluded. Unknown state
  and unloaded models remain candidates, not claims of readiness. Metadata is a
  registration snapshot, not continuous lifecycle telemetry.
- The legacy registry `status`, `size_bytes` and `vram_needed_mb` fields remain for
  compatibility; their old defaults are not new measurements. Consumers needing
  unknown-aware state/resources should use `model_metadata` or `eef models list`.
- Model failure tracking includes backend identity. New coordinator requests
  include backend selection; new node executors reject unsupported/unselected
  explicit backends rather than falling back. Old requests without a backend keep
  the previous local-GGUF-first behavior. Old nodes do not enforce this new backend
  field, so exact cross-backend dispatch requires an updated node.
- Text work uses `llm.infer`, vision work uses `vlm.analyze`; a vision-capable model
  is no longer automatically invoked as a vision operation for text-only work.

### Owner selection hints (subsequent unreleased increment)

Existing Ollama `selected[]` and GGUF `slots[]` may now include
`selection: {capabilities: [...], roles: [...]}`. Missing capabilities preserve
legacy adapter support; an empty list explicitly disables model capabilities.
Only subsets of the selected text/VLM adapter's supported capabilities are allowed.
Roles remain optional labels. Lifecycle and resources cannot be fabricated through
this selection object. New nodes project the hints into registration metadata and
enforce capability restrictions at execution, not just in eef's routing.

[Node model commands](COMMANDS.md) provide locked updates without editing JSON.
The development eef command facade can invoke the same selection operations on
connected, updated nodes; mutation requires node-local management approval,
rechecked under the node configuration lock. It never downloads or restarts
implicitly and does not use a legacy full-config save fallback.
The backend config format and auto-provider policy remain compatible; simultaneous
backend activation and general non-text/VLM executor configuration remain pending.
Older nodes ignore the new hints, so downgrade does not preserve enforcement.

Non-text/vision metadata (e.g. OCR, embedding, detection or STT/TTS) is representable
and inspectable, but this increment does not add those model executors, generalized
non-text/vision configuration, automatic role-based planning, default-LM bootstrap,
resource-aware admission, lifecycle controllers or execution leases. Existing
node permissions still govern execution. A reported capability is never a grant.

Explicit text/vision requests can require `constraints.model_role`, matching an
advertised role exactly. Missing or empty roles do not match; malformed role
constraints are rejected. Backend and role constraints also hold during tier
fallback. `eef models route` previews connected candidates using this filtering,
without loading a model or granting execution. Existing durable jobs now include
`generate_text` and `analyze_image` templates; these are deterministic requests,
not a natural-language interpreter or automatic role assignment.

## Tests

`tools/test-model-metadata.mjs` uses an isolated eef and node with a fake local
Ollama HTTP service. It tests versioned registration, owner inventory/CLI,
backend selection without fallback, and removal of model registrations after
selection is cleared and the fixture node restarts. It does not demonstrate real
inference or physical two-PC execution. Existing Rust tests cover legacy fallback,
validation, multi-capability filtering, snapshot replacement and backend-specific
failure isolation. See [status](STATUS.md) for recorded validation results.

Set `EEF_TEST_LEGACY_NODE_BINARY` to an owner-selected older node executable for
an isolated compatibility run with the new coordinator. That run verifies legacy
normalization but deliberately does not claim backend enforcement by the old node.
