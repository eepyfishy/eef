# Selecting models on a node

The EEF distribution contains no model and never downloads one. Model choice
belongs to the owner of each node.

`models.provider` controls the backend:

- `auto` (default) uses Ollama when its local API is reachable; otherwise it
  uses the bundled llama.cpp server.
- `ollama` requires the configured Ollama service.
- `llamacpp` always uses the bundled/configured llama.cpp server.

No backend is useful until the node owner selects an installed Ollama model or
provides a local GGUF path for a llama.cpp slot.

List the models already installed in the node owner's Ollama instance without
selecting or downloading anything:

```powershell
.\eefn.exe --config .\config\node.example.json --list-ollama-models
```

An owner can select models for one run without modifying the configuration:

```powershell
.\eefn.exe --config .\config\node.example.json `
  --select-model "your-text-model-id=text" `
  --select-model "your-vision-model-id=vlm"
```

Those IDs are placeholders, not defaults. Persistent choices use the JSON
configuration below.

For an Ollama model that is already installed on a node, add an explicit entry
to that node's JSON configuration:

```json
{
  "models": {
    "ollama": {
      "base_url": "http://127.0.0.1:11434",
      "selected": [
        {"model_id": "your-installed-text-model", "modality": "text"},
        {"model_id": "your-installed-vision-model", "modality": "vlm"}
      ]
    }
  }
}
```

An empty or missing `selected` array advertises no Ollama models and does not
even query Ollama. A selected model is advertised only if it is present in
Ollama's installed-model list. EEFN rejects requests for an unselected model or
for the wrong modality.

llama.cpp slots are also opt-in:

```json
{
  "models": {
    "llamacpp": {
      "binary": "tools/llama-server.exe",
      "slots": [
        {
          "model_id": "owner-defined-id",
          "model_path": "models/owner-selected.gguf",
          "port": 8082,
          "gpu_layers": 0,
          "context": 4096
        }
      ]
    }
  }
}
```

EEF receives these advertisements over the authenticated node link. It ranks
eligible providers by capability, modality, load, latency, hardware,
constraints, and health; it does not contain a default model name.

## Command-based installation (development after v0.4.0a3)

The running node now exposes installed-model inspection, explicit installation,
receipt-scoped progress and cancellation through `eefn models` commands. They
reuse the same installer as the API/UI, with no automatic selection, loading or
restart. This is groundwork for default lightweight-model setup, not completed
bootstrap. See [command syntax and lost-reply limits](COMMANDS.md).

## Model startup (development after v0.4.0a3)

If the configured llama.cpp group fails startup, EEFN stops any partially
started children it owns, marks that group unavailable for execution and
continues its deterministic node connection. Saved model selections and backend
preference are retained; there is no silent switch to another model/provider.
This currently disables the whole local group on startup failure, not just one
slot. Model startup now runs alongside registration, heartbeats and commands;
the five-minute group health deadline does not block the core connection.
Restart/shutdown cancels the scoped startup future and cleans up its children.
There is no detached loader that can continue spawning after a restart.

The selected group remains inspectable in model inventory with `unloaded`,
`loading`, `ready` or `error` lifecycle metadata. It is advertised as unavailable
until every configured slot passes startup health checks. Non-ready models do
not advertise inference capabilities, and direct inference also checks readiness.
Readiness changes refresh registration on the existing authenticated connection,
without reconnecting or resetting unchanged capability scheduling counters.
Saved selections are never removed on failure and no mutation is replayed.

This is group startup state, not continuous health monitoring or measured active
usage. Independent slot loading, on-demand loading, idle unload and a default LM
bootstrap remain future work. A process failure after startup is not yet reflected
in lifecycle metadata; normal inference errors still propagate to the caller.
Provider discovery and Python/plugin inspection retain their existing bounded
startup work. Released v0.4.0a3 still waits for GGUF startup before registration;
these changes are not in its published installers.

Missing/unusable Python or a failed built-in plugin similarly withholds the
affected optional capabilities without preventing registration. A bounded
interpreter probe runs only when Python capabilities/plugins were configured.
No camera, microphone or audio invocation is performed by that probe. Importing
owner-selected plugins still runs their existing inspection code.

Node status and `eefn network diagnose --json` expose `startup_issues`: fixed,
bounded codes `llamacpp_startup_failed`, `python_runtime_unavailable`,
`builtin_plugin_unavailable`, `custom_plugin_unavailable`. These describe the
latest startup attempt, not continuous health or proof of working inference or
hardware. Raw exception text and file paths are excluded from diagnostics;
local application logs may contain troubleshooting details. Fix the saved
configuration and explicitly restart; normal approval is still required for
remote restart. Successful startup clears the previous attempt's issue codes.
