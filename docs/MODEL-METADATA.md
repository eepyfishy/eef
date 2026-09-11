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

- No `model_metadata`: normalize explicit `text`/`vlm` labels using the previous
  compatibility rules. Unknown/missing labels provide no capability hints.
- Present `model_metadata`: it is authoritative for capability filtering, even
  when the legacy label disagrees. Invalid metadata is never silently downgraded.
- New Ollama and local-GGUF advertisements emit this object alongside the legacy
  fields, based on the existing selected modality/projector configuration. They
  do not infer capabilities from model names or claim lifecycle/resource telemetry.
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

Non-text/vision metadata (e.g. OCR, embedding, detection or STT/TTS) is representable
and inspectable, but this increment does not add those model executors, generalized
model-selection configuration, role-based scheduling, default-LM bootstrap,
resource-aware admission, lifecycle controllers or execution leases. Existing
node permissions still govern execution. A reported capability is never a grant.

## Tests

`tools/test-model-metadata.mjs` uses an isolated EEF and node with a fake local
Ollama HTTP service. It tests versioned registration, owner inventory/CLI,
backend selection without fallback, and removal of model registrations after
selection is cleared and the fixture node restarts. It does not demonstrate real
inference or physical two-PC execution. Existing Rust tests cover legacy fallback,
validation, multi-capability filtering, snapshot replacement and backend-specific
failure isolation. See [status](STATUS.md) for recorded validation results.

Set `EEF_TEST_LEGACY_NODE_BINARY` to an owner-selected older node executable for
an isolated compatibility run with the new coordinator. That run verifies legacy
normalization but deliberately does not claim backend enforcement by the old node.
