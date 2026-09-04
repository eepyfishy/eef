# EEF

Native Rust implementation of the EEF coordinator and EEFN node, with bundled
CPython for optional plugins. The transport, crypto, routing, memory, task
engine, model routing, dashboard/API, firmware generation, and updates are
implemented in Rust. See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and the
honest conformance matrix in [`docs/STATUS.md`](docs/STATUS.md).

## Windows releases

The build creates exactly two release archives:

- `eef-windows-x86_64.zip` — coordinator, dashboard, and bundled CPython.
- `eefn-windows-x86_64.zip` — standalone node, dashboard, bundled CPython, and
  the llama.cpp server runtime.

Neither archive contains or downloads a model. Each node owner selects an
already-installed Ollama model or supplies a local GGUF file as described in
[`docs/NODE_MODELS.md`](docs/NODE_MODELS.md). Automatic provider selection uses
Ollama when reachable and otherwise uses llama.cpp.

After extracting either archive, run `install.ps1`. The installer asks for a
dedicated installation directory and offers startup as an opt-in choice. The
download/extraction directory is never treated as the installed location.

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
enables them.

## Rebuild

The storage-aware bundler downloads temporary, checksum-verified LLVM-MinGW,
official CPython, and official llama.cpp runtime packages. It runs tests,
creates the two release ZIPs, and removes build output afterward:

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
