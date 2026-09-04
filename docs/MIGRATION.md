# Migration from the Python repositories

The original trees were not copied into this repository. The `legacy` note
records that preservation decision without publishing machine-specific paths or
duplicating their files.

## Component mapping

| Python subsystem | Rust subsystem |
| --- | --- |
| `eef.core.runtime` | `crates/eef/src/runtime.rs` |
| event bus and world state | `event.rs`, `world.rs` |
| identity/mutable/working memory | `memory.rs` with compatible SQLite tables |
| capability registry/adapters | `capability.rs`, `adapters.rs` |
| planner/task engine/dispatcher | `task.rs` |
| assistant rules and brain loop | `assistant.rs`, `brain.rs` |
| FastAPI surface | Axum routes in `api.rs` |
| node server/client/crypto | `crates/eefn/src/server.rs`, `client.rs`, `crypto.rs` |
| node engine/model/update | `engine.rs`, `model_server.rs`, `updater.rs` |
| ESP generation/OTA | `crates/eefn/src/firmware.rs` plus coordinator service |

Unlike the Python coordinator, the Rust coordinator has no local model backend.
It learns only the models selected and advertised by connected nodes, then
routes every LM/VLM request through the encrypted node protocol.

The node protocol remains newline-framed encrypted JSON using HKDF-SHA256 with
salt `eef-node-salt`, info `eef-node-v1`, AES-256-GCM, and HMAC-SHA256. Auth now
also enforces clock skew, nonce replay protection, protocol version, duplicate
node rejection, and authenticated node-ID binding.

Configuration remains YAML for the coordinator and JSON for nodes. Secrets are
not migrated into files: set `EEF_NODE_PSK`, then put the same value in each
node's protected configuration.

Python adapters use a smaller contract:

```python
CAPABILITY = "example"
ACTIONS = ["run"]
PARAMS_SCHEMA = {"value": {"type": "string", "required": True}}

def handle(action, params):
    return {"value": params["value"]}
```

List a trusted plugin path under `python.plugins`. No directory is scanned and
no plugin is loaded implicitly.
