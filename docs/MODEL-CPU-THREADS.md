# Explicit local-model CPU settings (unreleased after alpha.6)

Owners can request a bounded worker count for each selected llama.cpp GGUF model
without editing JSON, reselecting its file, changing its role or loading it.
Both node and coordinator clients use the same typed model command service.

```powershell
eefn models cpu-threads --model MODEL_ID --threads 4 --json
eefn models show --json
eefn restart --wait-seconds 60 --json

# Remote changes require management approval saved locally on the target node.
eef node models --node NODE_ID cpu-threads --model MODEL_ID --threads 4 --json

# Explicitly remove the configured count; does not restart the node.
eefn models cpu-threads --model MODEL_ID --auto --json
```

`select-gguf` also accepts `--cpu-threads N` when selecting an existing file.
Reselecting that model without this option preserves its existing explicit count;
use `cpu-threads --auto` to clear it. Existing configurations lacking the field
retain backend defaults. Ollama is externally managed and is not tuned through
this command; it does not accept an Ollama/backend switch or infer a model ID.

## Saving and applying

The command requires exactly one existing GGUF selection with the requested ID.
It merges only `cpu_threads` under the existing configuration lock, preserving
paths, internal ports, projector, roles/restrictions, provider, permissions,
credentials and custom configuration. Missing/duplicate targets, invalid values,
unknown command fields or remote management refusal do not save a change. The
wire command requires an explicit nullable `threads` field; omission is not a
silent reset. Values are integers 1-256; `null` means an explicit reset. This is
a validation bound, not a machine-specific recommendation.

Saving neither loads a model nor restarts, executes work or changes providers.
Repeated identical settings are no-ops. Inspect `saved_selections` separately
from `registered_models` and `restart_required`; the former is intended settings,
not current readiness. Applied model advertisements and coordinator inventory
include an explicit `cpu_threads` only when configured. Absent fields on legacy
or default-configured models do not establish an observed worker count.
Lifecycle/availability remain independently reported, including startup failure.
Restart is separate and applies all already-saved pending node configuration.

Changing the saved model configuration continues to fence existing model grants
until the usual restart/replanning checks succeed. It does not widen or reuse an
execution grant. Coordinator management permission is rechecked at application,
not inferred from a prior inspection, model selection or a clear natural-language
request. The existing configuration backup policy still applies.

## Backend settings, not resource enforcement

When configured, the launcher passes the same count to both `--threads` and
`--threads-batch`, for generation and prompt/batch processing respectively.
These flags are verified in the bundled runtime's help and
[pinned upstream argument implementation](https://github.com/ggml-org/llama.cpp/blob/c1d0e7a00/common/arg.cpp).
An explicit CLI count takes precedence over backend environment defaults.
Unspecified counts retain the backend's defaults/environment; no count is guessed.

The launcher now always passes configured GPU layers, including `-ngl 0` for the
existing CPU choice. Omitting that zero previously left the choice to a custom
runtime's defaults/environment despite advertising CPU hardware. GPU acceleration
remains optional and explicitly configured nonzero values remain supported.

Worker settings are not CPU-percent, affinity, total-process/thread, RAM, VRAM,
bandwidth or power quotas. HTTP/native/media threads and backend overhead may
consume additional resources. They do not prove native termination, stop ACK,
physical resource budgets or lightweight performance on the second PC. Automatic
thread optimization, default-model quality and full resource enforcement remain
open roadmap gates; no automatic bootstrap is enabled here.

## Confirmation and compatibility

Successful replies confirm the exact saved node/model/count, not completed
restart, worker measurement or model quality. Lost/malformed/substituted replies
report an unknown outcome without automatic replay, config fallback, provider
switch or restart. Local uncertainty uses
`local_model_cpu_threads_reply_unconfirmed`; HTTP 404 reports
`unsupported_command`. Configuration commands still have no durable receipt
ledger. Inspect the saved selection before deciding whether to send a new change.

Published alpha.6 installers do not contain these commands. Use matching new
coordinator/node builds for testing; no installer or feed is replaced by this
source increment. UI assets and personal/physical nodes are unchanged.

## Development validation

Five added Rust tests cover CLI choice/bounds, explicit nullable wire input,
exact/duplicate target checks, failed-update preservation, settings/role
preservation, remote owner refusal, exact reply confirmation, optional bounded
advertisements and both worker-pool arguments. All 422 workspace/all-target tests
pass with default features and `--no-default-features`.
After the final coordinator command-echo change, both full suites are rerun.
Fresh binaries pass saved-UI/API command metadata (`model-metadata-ryecSb`),
first-run/browser (`first-run-CZZsJm`), verified-artifact real CPU inference
(`real-model-H7NFNK`) and the source backend/browser guard. An earlier API-only
metadata run also passes (`model-metadata-gnZgmm`); fixture directories remain
ignored development evidence, not release artifacts.

The expanded metadata fixture checks approved/denied coordinator commands,
lost/substituted replies after real saves, HTTP 404 without fallback, explicit
reset and unchanged runtime/roles/provider/inference count. Its tiny test file is
never loaded as a model. The separate real-model fixture selects an existing
verified artifact through public commands, saves two threads without a load,
restarts explicitly, observes saved/applied settings and coordinator inventory,
and checks the owned child command line for both worker flags and zero GPU layers.
Reviewed real CPU inference, new runtime identity, retained results without
replay and model withdrawal remain checked. Process arguments establish launch
settings, not measured thread usage or total resource enforcement.

An initial saved-UI metadata run returned a truthful `node_busy`/`not_started`
response during repeated remote hints at work capacity one. That invocation is
not counted as passing; the standalone rerun passes. The exact admission race
is not diagnosed or fixed here, no automatic retry is added and no admission
assertion is relaxed. Transient capacity behavior remains a reliability follow-up.
Local logical nodes, synthetic data and one CPU PC do not meet the physical
multi-node, resource-budget, continuous-pipeline or stable-release gates.
