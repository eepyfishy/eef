EEFN node for Windows

v0.4.0a6 backend alpha. See TWO_PC_TEST.md. Use eefn.exe network diagnose
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

Command-based repair (unreleased, publisher-enabled packages):
  eefn repair --check
  eefn repair --approve PROPOSAL_UUID
  eefn repair --status
Check displays the public publisher source and retains a proposal only; it does
not download a package or restart. Approve selects one exact verified repair;
restart separately. No URL, hash, port or JSON entry is needed when the publisher
supplies a source hint. Hint-less builds with these commands can use --manifest
explicitly; older binaries may need an upgrade for command support.
For a prerelease, add --allow-prerelease to check/approve only; this never opts
automatic updates into alphas. Review the publisher before approval. Repair is
not a health certificate or a reason to bypass an antivirus detection.

Missing-file maintenance (unreleased; off by default):
  eefn repair-setup show
  eefn repair-setup configure --mode prompt
  eefn repair-setup run
  eefn repair-setup approve --operation-id ATTEMPT_UUID
  eefn repair-setup configure --mode off
Requires a managed bundle and reviewed public HTTPS source (packaged hint or
--manifest). One attempt per saved policy identity; interrupted work never
resumes automatically. Auto mode additionally needs --allow-automatic-repair.
All prerelease runner activity is blocked. No automatic upgrade/restart/model
setup is granted. Missing-file presence and selection are not runtime health.
