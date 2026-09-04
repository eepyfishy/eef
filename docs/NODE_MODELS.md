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
