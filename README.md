# eef

**eve's extensible framework** is being built as a lightweight foundation that
users expand with the capabilities, models and integrations they choose. eve is
the main motivation and intended flagship application; eef operates independently
of eve. Core connection, status, management and recovery remain usable without
working inference. The modular-engine and optional-assistant direction is recorded
in [the architecture notes](docs/ARCHITECTURE.md); the v0.5 design remains gated
on stable v0.4 and explicit owner authorization.

Start with the [user guide](docs/USER_GUIDE.md) for installation, node and model
setup, a complete request walkthrough, and troubleshooting.

[v0.4.0a7](https://github.com/eepyfishy/eef/releases/tag/v0.4.0a7), internal
version `0.4.0-alpha.7`, is an opt-in Windows prerelease for two-PC testing.
It includes wizard installers, command-first private node enrollment, and
[reviewed requests](docs/EXECUTION-BOUNDARY.md) through the running node.
Automatic-update feeds are paused and feed-based updates reject prereleases.
See the [active core roadmap](docs/ROADMAP-CORE.md) for the remaining stable
release gates and [alpha.6 notes](docs/RELEASE-0.4.0a6.md) for that older build.
The builds are unsigned. Both v0.3.2 installers have unresolved Defender reports
on a second PC; see [the investigation](docs/ANTIVIRUS.md). A passing local scan
is not a Microsoft false-positive verdict. Do not bypass antivirus detections.

Native Rust implementation of the eef coordinator and eef node (`eefn`), with bundled
CPython for optional plugins. The transport, crypto, routing, memory, task
engine, model routing, dashboard/API, firmware generation, and updates are
implemented in Rust. See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and the
honest conformance matrix in [`docs/STATUS.md`](docs/STATUS.md).

## Windows releases

The v0.3.0 node installer has a reported Microsoft Defender detection. Keep
that artifact quarantined; see [the investigation](docs/ANTIVIRUS.md) for the
maintenance build's validation and limitations.

The build creates exactly two self-extracting per-user installers:

- `eef-installer.exe` — coordinator, dashboard, and bundled CPython.
- `eefn-installer.exe` — standalone node, dashboard, bundled CPython, optional
  media adapters, and the llama.cpp server runtime.

Neither installer contains or downloads a model. Each node owner uses Models
to explicitly install a model, select an already-installed Ollama model, or
choose a local GGUF file as described in
[`docs/NODE_MODELS.md`](docs/NODE_MODELS.md). Automatic provider selection uses
Ollama when reachable and otherwise uses llama.cpp.

Double-click an installer. The graphical setup lets you choose a dedicated
folder (default `%LOCALAPPDATA%\eef` or `%LOCALAPPDATA%\eefn`), opt into startup,
and choose whether to open the app when installation finishes. `--install-dir` supports
another dedicated location. The
installer refuses to use its download directory and preserves existing config
on upgrade. These open-source builds are currently unsigned, so Windows may
show a SmartScreen warning. Do not bypass an antivirus detection.

For a first installation, follow [Installation and connection](docs/user/INSTALLATION.md).
Install and launch **both apps on the same PC**, then use the local enrollment
steps to pair them. Nodes keep their identity when renamed or restarted.
eef's Nodes page can request configuration changes; the node owner must
approve them locally or explicitly allow remote management.

For development, start the coordinator from PowerShell:

```powershell
$env:EEF_NODE_PSK = '<a random secret of at least 12 characters>'
.\eef.exe --config .\config\default_identity.yaml
```

The accessible coordinator dashboard is at `http://127.0.0.1:51334/`; the node
dashboard is at `http://127.0.0.1:51336/`. Both expose the full corresponding
configuration and an optional startup toggle.

eef is not a node and remains `waiting_for_node` until at least one eefn is
connected. eefn remains a node without eef, but network orchestration requires
at least one eef. With multiple coordinators, configure endpoint priorities on
nodes; the highest-priority reachable eef is attempted first.

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

It refuses to overwrite existing outputs. There is no reserved 5 GB floor;
installers and model downloads check the space needed for their payloads.
Development details are in
[`docs/BUILD.md`](docs/BUILD.md).

Update behavior is configurable as `off`, `prompt` (default), or `auto`; see
[`docs/UPDATES.md`](docs/UPDATES.md). Official packages default to this
repository's feed, and forks can replace or disable it in the dashboard.

## License

eef is MIT licensed. Bundled third-party runtimes retain their own notices.
