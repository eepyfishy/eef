# Granite 4.0 1B candidate: predeclared evaluation (2026-10-25)

The current interpreter/default requirement is still unmet. Evaluate another
publisher-provided small model, not another prompt variation or weaker rubric.
The manual catalog option is not an approved default, automatic setup choice,
download on startup, or claim of model quality. The first catalog entry and all
production prompt/schema/validator/permission behavior remain unchanged.

## Provenance and admission

Use IBM's [pinned GGUF metadata](https://huggingface.co/api/models/ibm-granite/granite-4.0-1b-GGUF/revision/b27c2fe3f211b7f44e80fa620177aea371099aaa?blobs=true):
`granite-4.0-1b-Q4_K_M.gguf`, 1,023,645,440 bytes, SHA-256
`22ec0f9cc99a90185312de3c882c84e7bd6789bdd050389844380a01a831d7f1`.
The [pinned upstream card](https://huggingface.co/ibm-granite/granite-4.0-1b/blob/6a7381ba1f54d684ff508d991aeb7dc580157103/README.md)
and quantized repository declare Apache-2.0. These are publisher declarations and
provenance, not an independently verified training lineage or legal opinion.
No weights are bundled or redistributed in the source change.

The [Ollama alias](https://ollama.com/library/granite4:1b) is a separate mutable
provider artifact; its reported size/quantization differs. Do not transfer the
GGUF's hash, memory measurements or quality conclusions to that alias.

At admission about 6.9 GB is free. Download at most this one 1.02 GB artifact
through the existing explicit public plan/install/status workflow in a scratch
node; verify exact bytes/hash before reuse. Reuse existing trusted CPU runtime
and app builds. No extra runtime, model copies, installed-node changes, physical
node updates or update-feed changes. Preserve failed/interrupted evidence.

## Fixed comparison before candidate inference

Run the existing real-model lifecycle/owner-workflow fixture first. Reuse its
verified cache for both `original-v1` and `generalization-v1` quality suites.
Use current production temperature-zero defaults, explicit disabled-thinking
hint, 4,096-token context and existing 512-token output cap. Do not alter prompt,
generation schema, validator, frozen cases, scorer or expected answers. Record
binary/runtime/model hashes, applied sampling, timing and reference fingerprints.
No automatic inference retry, response repair or post-result profile selection.

Separate harness completion, strict proposal acceptance, label matches and
goal/question faithfulness. Inspect all forty returned proposals, not only the
aggregate score. Any missing required clarification, invented target/action,
copied example goal, wrong intent/workload or failed bounded completion rejects
default candidacy under this screen. Even a clean screen would only justify
further independent/multilingual/physical evaluation, not default approval.
`default_approved` remains false. Automatic onboarding and full stable-release
acceptance remain separate requirements.
