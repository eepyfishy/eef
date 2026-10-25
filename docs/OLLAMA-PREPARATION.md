# Explicit shared-Ollama preparation (unreleased)

An applied Ollama selection can be cold even when setup reports it configured.
The new owner command separates model loading from the existing 20-second local
preview deadline. It uses the existing lifecycle queue/worker and owner service,
not a browser controller or a second runner. It does not run automatically yet.

```text
eefn models installed --json
eefn model-lifecycle show --json
eefn model-lifecycle --runtime RUNTIME_UUID prepare-ollama --model INSTALLED_MODEL_ID --digest PROVIDER_DIGEST --operation-id OPERATION_UUID --json
eefn model-lifecycle show --operation-id OPERATION_UUID --json
eefn model-lifecycle --runtime RUNTIME_UUID cancel --operation-id OPERATION_UUID --json
```

Use the exact model name and digest from `inventory.ollama_models` and generate
one UUID for each distinct preparation intent. These are provider identity
reports, **not verified local artifact hashes**. The applied provider must be
Ollama (explicitly or selected by `auto`), the model must already be selected for generation, and saved model/policy
state must still match the applied runtime. Saving a different selection/context
requires explicit restart first. Zero retention is refused because it requests
immediate unloading; positive, inherited or explicitly indefinite retention keeps
its existing meaning. No download, selection, provider switch or restart occurs.

Approved coordinator commands use `eef node model-lifecycle --node NODE_ID ...`
with the same arguments. Only the node owner can enable remote management.
The common schema-1 `LifecycleRequest`, owner HTTP routes and authenticated
`node.configure` transport are unchanged. `command.operation` is `prepare_ollama`
with `model`, `digest` and `operation_id`. Status/cancel use existing reserved
control admission; preparation uses normal workload admission before joining the
existing bounded model lifecycle queue. No new port or inbound provider API is
exposed. Older nodes reject the new operation; adapters do not fall back.

## What preparation does

It checks a unique installed tag's exact digest and positive size, refuses
reported cloud/remote model aliases, then sends an empty-message `/api/chat`
request with the **applied** context and retention preferences. There is no user
prompt, system message, tool, sampling or thinking input. Ollama documents
[empty-request preloading](https://docs.ollama.com/faq#how-can-i-preload-a-model-into-ollama-to-get-faster-response-times).
No pull/create/delete/stop/unload endpoint or native process is invoked.

Completion requires an explicit `done:true`/`done_reason:"load"` reply without
generated text/tool calls, the same installed tag afterward and one matching
resident digest from [`/api/ps`](https://docs.ollama.com/api/ps). When the provider
reports context, it must match an explicit applied context; absent context stays
unknown. Metadata bodies use the existing 4 MiB bound; the whole operation uses
the applied lifecycle `load_timeout_seconds`, including inspections. Current
authority is rechecked before network stages and before confirming completion.
No configuration lock is held across provider waits. Revocation after a request
has left the node cannot undo that request's shared-service effects.

## Receipts and cancellation

The command acknowledgement means queued/inspected, not loaded. Poll the same
receipt until terminal. Successful preparation adds:

```json
{
  "phase": "completed",
  "shared_service_effects_possible": true,
  "preparation": {
    "provider_reported_digest": "EXACT_PROVIDER_DIGEST",
    "observed_context_tokens": 4096
  }
}
```

All preparation receipts conservatively carry `shared_service_effects_possible:
true`, including queued, failed, cancelled and interrupted receipts; only completed
ones carry an observation. `ollama_preparation_failed` is static and does not
expose backend errors, generated text or private paths. A completed receipt is
historical evidence of one observed preparation, not a live readiness lease.

Exact command/operation reuse reads the retained receipt and **never sends
another preload**. Conflicting reuse fails. Queue and retained-receipt limits are
shared with owned GGUF operations; receipts are runtime-local, not persisted or
evicted to admit more work. Restart interrupts pending operations and never
resumes/replays them. After missing/malformed replies, inspect the original
operation ID rather than create an automatic replacement.

Cancellation/timeout drops eef's wait/request, not ownership of Ollama. It never
requests forced unloading, stops the shared service, or
proves that server work/VRAM use ended. Provider scheduling may evict other models or change other
residency; the operation offers no exclusive lease or RAM/VRAM quota. These
effects remain possible after failures and node restart. No model-quality,
successful inference, inference permission, execution grant or future residency
is implied. Preview still uses its unchanged deadline and strict validation.

`supported` continues to mean owned-GGUF lifecycle support. Ollama reports add
`ollama_preparation_supported:true`; owned-model snapshots stay empty and never
invent an owned PID. Unused fields are omitted from legacy GGUF receipts/reports.
Older strict adapters may reject the new Ollama report shape; update both apps
before using preparation, without retry/fallback through an older coordinator.

Full first-run automation, live readiness in setup, default interpreter quality,
two-backend quality comparison, physical acceptance and stable v0.4.0 release
remain separate unfinished requirements.

## Verification: 2026-10-17

Both locked/offline workspace/all-target suites pass **726 tests plus one ignored
each**, including no-dashboard. Fresh binaries/examples build. Six new Rust
tests cover command/digest validation, actual applied `auto` behavior, cancellation,
bounded admission, owner/config checks, strict observations and legacy omission.
Public synthetic fixture `ollama-preparation-u1C3pA` checks local/approved remote
commands, denial before provider mutation, applied context/retention, exact
receipt reuse, missing/duplicate/zero-size/cloud/changed tags, post-load identity,
absent residency, wrong context, generated-text rejection, timeout/cancel,
revocation after shared effects, stale-runtime refusal and zero-retention refusal
without sending an unload. Observer synthetic
checks also cover separate preparation followed by preview. These are same-PC
fake-provider tests, not physical two-PC or real-model quality evidence.
Fresh existing regressions `request-preview-bpRNrZ` and `model-policy-YKkcln`
pass strict preview behavior and owned-GGUF lifecycle/policy preservation.

The explicit opt-in real run `real-ollama-context-LwtmHF` uses the owner's existing
Ollama 0.40.4 / Qwen3-VL tag, with no pulls, model copies or service changes. Initial
resident inventory is empty. One public `prepare-ollama` command completes in
**12,982 ms**, with matching pre/post reported digest
`901cae73216286ea8c5aba8b46d307ff7188f737285ec500c795a12f05225d28` and reported
resident context **4,096**. It has no owned PID or inference authority.

The following single synthetic public preview **fails** with
`interpretation_timeout` after **20,026 ms**, without retry. The provider still
reports the same resident digest/context, 6,329,886,308 resident bytes and
3,469,087,866 VRAM bytes. These are provider reports, not measured per-process
quotas or verified artifact bytes. No proposal/quality score is obtained; this
does not establish the underlying generation-time cause or reliable warm
interpretation. The overall fixture remains `passed:false`, while its separate
preparation receipt records the completed observation. Saved configuration is
unchanged. Only test-owned node/CLI processes stop; ordinary shared-service
retention/expiry remains in charge, without forced unloading.
A later read-only resident inventory is empty; that is an observation, not a
claim of exclusive control over shared expiry or other clients.

The tested node executable SHA-256 is
`e9dec6b4845bbaaf213a82c007efff7f2ad0f36d34975aef005305e722e63a16`.
Preparation now has real provider evidence, but it does **not** solve the earlier
cold-preview failure or default interpretation/latency acceptance by itself.
Keep the failed earlier cold run intact; see [previous checkpoint](OLLAMA-CONTEXT.md#measured-checkpoint-2026-10-17).
No installed/physical node, UI, release, update feed or model default changes.

To opt into a new isolated preparation-plus-preview check, explicitly supply an
installed model and loopback origin:

```powershell
$env:EEF_TEST_ALLOW_SHARED_OLLAMA = '1'
node tools/test-real-ollama-context.mjs MODEL_ID http://127.0.0.1:11434 --prepare
```

This may load the shared model and use RAM/VRAM. It refuses unrelated observed
resident models and insufficient inspected free system RAM, but cannot reserve
resources against other clients. One preparation and one preview are allowed;
failure is preserved without retry. This is not the full quality evaluator.
