# EEF

Native Rust implementation of the EEF coordinator and EEFN node, with bundled
CPython for optional plugins. The transport, crypto, routing, memory, task
engine, model routing, dashboard/API, firmware generation, and updates are
implemented in Rust. See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and the
honest conformance matrix in [`docs/STATUS.md`](docs/STATUS.md).

## Windows releases

The build creates exactly two self-extracting per-user installers:

- `eef-installer.exe` — coordinator, dashboard, and bundled CPython.
- `eefn-installer.exe` — standalone node, dashboard, bundled CPython, optional
  media adapters, and the llama.cpp server runtime.

Neither installer contains or downloads a model. Each node owner selects an
already-installed Ollama model or supplies a local GGUF file as described in
[`docs/NODE_MODELS.md`](docs/NODE_MODELS.md). Automatic provider selection uses
Ollama when reachable and otherwise uses llama.cpp.

Double-click an installer. It uses `%LOCALAPPDATA%\EEF` or
`%LOCALAPPDATA%\EEFN`, offers startup as an opt-in choice, and launches the
local dashboard. `--install-dir` supports another dedicated location. The
installer refuses to use its download directory and preserves existing config
on upgrade. These open-source builds are currently unsigned, so Windows may
show a SmartScreen warning.

For development, start the coordinator from PowerShell:

```powershell
$env:EEF_NODE_PSK = '<a random secret of at least 12 characters>'
.\eef.exe --config .\config\default_identity.yaml
```

The accessible coordinator dashboard is at `http://127.0.0.1:51334/`; the node
dashboard is at `http://127.0.0.1:51336/`. Both expose the full corresponding
configuration and an optional startup toggle.

EEF is not a node and remains `waiting_for_node` until at least one EEFN is
connected. EEFN remains a node without EEF, but network orchestration requires
at least one EEF. With multiple coordinators, configure endpoint priorities on
nodes; the highest-priority reachable EEF is attempted first.

Sensitive node capabilities remain disabled until the node owner explicitly
enables them. The implemented capability catalog and request shapes are in
[`docs/CAPABILITIES.md`](docs/CAPABILITIES.md).

For a practical setup and failover check with two Windows PCs, follow
[`docs/TWO_PC_TEST.md`](docs/TWO_PC_TEST.md).

## Rebuild

The storage-aware bundler downloads temporary, checksum-verified LLVM-MinGW,
official CPython, and official llama.cpp runtime packages. It runs tests,
creates the two installer executables, and removes build output afterward:

```powershell
.\tools\bundle-windows.ps1
```

It refuses to overwrite existing outputs, requires 8 GB free at startup, and
preserves a 5 GB safety floor. Development details are in
[`docs/BUILD.md`](docs/BUILD.md).

Update behavior is configurable as `off`, `prompt` (default), or `auto`; see
[`docs/UPDATES.md`](docs/UPDATES.md). Official packages default to this
repository's feed, and forks can replace or disable it in the dashboard.

## License

EEF is MIT licensed. Bundled third-party runtimes retain their own notices.
