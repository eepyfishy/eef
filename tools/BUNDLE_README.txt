EEF native Windows bundle

1. Open PowerShell in this directory.
2. Set a node transport secret:
   $env:EEF_NODE_PSK = '<a random secret of at least 12 characters>'
3. Start the coordinator:
   .\eef.exe --config .\config\default_identity.yaml
4. Configure config\node.example.json with the same secret, then start a node:
   .\eefn.exe --config .\config\node.example.json

No AI model is included or downloaded. Add only models already installed on a
node to models.ollama.selected in that node's JSON configuration.
Use `eefn.exe --list-ollama-models` to list installed IDs, or repeat
`--select-model "MODEL_ID=text"` / `--select-model "MODEL_ID=vlm"` to select
models for one run without editing the configuration.

The shared CPython runtime is in .\python. Python plugins do not replace the
Rust core and are loaded only when listed under python.plugins in the YAML.
Keyboard, screen capture, filesystem writes, shell, and hardware capabilities
are denied until explicitly allowed in capability_policy.
