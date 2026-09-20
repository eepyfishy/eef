EEFN node for Windows

v0.4.0a5 backend alpha. See TWO_PC_TEST.md. Use eefn.exe network diagnose
--json for a minimal local report, or network show/set/peers for network commands.
No reports upload automatically. Pairing codes and raw config must stay private.

This directory was created by eefn-installer.exe. Open EEF Node from Start >
EEF. Do not move the app into Downloads. Startup is optional in Settings.

Launch EEF on this PC and the node connects automatically under your Windows
account. Home shows the real connection state. Use Permissions to choose what
to share, Models to install and select a model, and Apply and restart to make
saved settings active. EEFN remains a node without a connected
EEF, although coordinated network work requires at least one EEF.

No AI model is included or downloaded during setup. Provider mode auto uses an existing
Ollama service first and otherwise uses the bundled llama.cpp server with an
owner-selected GGUF file. Downloads only begin when you request them in Models.
Network accepts a private connection code from another PC's EEF. Remote settings
requests need local approval unless you explicitly allow management in Settings.

Use Home to ask your network and Jobs to review saved planned actions. Stop does
not undo completed actions; work already sent to a node may finish. This alpha
does not provide live media relay, Wi-Fi/Bluetooth control, or a peer mesh.
The builds are unsigned. Do not bypass a Defender detection to install them.
Feed-based updates reject alpha/beta/RC builds, including automatic updates.
Automatic-update feeds are paused until a stable release is published.
Alpha.5 supports separately approved, exact-artifact remote updates when Remote
updates is enabled. This does not grant process/file access or subscribe to alphas.
Use --no-ui for API-only operation; models, jobs, connection and restart commands
operate through the running node without a browser or local language model.

Models supports installed/inspect/install-plan/install/download-status and
cancel-download commands. Downloads never select or load a model automatically.
requests preview --text TEXT --json requires exactly one applied, owner-selected
request_interpreter model. It returns a proposal only, never dispatches work,
and is not a substitute for deterministic commands or authorization.
