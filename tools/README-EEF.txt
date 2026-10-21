EEF coordinator for Windows

v0.4.0a6 backend alpha. TWO_PC_TEST.md describes private-network pairing and
safe two-PC checks. Commands: eef.exe diagnostics --json; eef.exe diagnostics
--node NODE_ID --samples 5 --json; eef.exe invite --address HOST:PORT --json.
Connection codes are secret. Reports are local only; no automatic analytics upload.

This directory was created by eef-installer.exe. Open EEF from Start > EEF.
Do not move the app into Downloads. Startup is optional in Settings.

EEF is a coordinator, not a node. It waits for at least one EEFN node before it
becomes operational. Install and launch the EEFN node app on this PC too.
It connects automatically under your Windows account. No secret or address
needs typing for same-PC use. Network can create a private connection code
for a node on another PC. Nodes > Configure node requests its settings.

No AI model is included. EEF routes model work only to owner-selected models on
connected nodes. Update policy defaults to prompt and is configurable.
Use Settings > Restart now to apply saved changes or an installed update.

This alpha provides saved one-shot jobs, not recurring jobs or safe coordinator
clustering. Use the node app's Jobs page to review, pause, resume or stop work.
Interrupted jobs never resume automatically. Already-sent actions may finish.
Alpha.6 checkpoints interrupted jobs before closing the node gateway, so an
uncertain remote action is not mislabeled as an ordinary failure at shutdown.
Automatic-update feeds are paused until a stable release is published.
Feed-based updates reject alpha/beta/RC builds, including automatic updates.
Use --no-ui to run commands and services without the browser interface.
eef node update supports one explicitly approved, version/hash/size-pinned
artifact on alpha.5+ nodes, without changing their automatic-update policy.

Local coordinator repair (unreleased, publisher-enabled packages):
  eef repair --check
  eef repair --approve PROPOSAL_UUID
  eef repair --status
Check displays the public publisher source and retains a proposal without a
package download or restart. Approve selects one exact verified repair; restart
separately. With a packaged source hint, no URL, hash, port or JSON entry is
needed. Hint-less builds with these commands can use --manifest explicitly;
older binaries may need an upgrade for command support. Prerelease check/approve
requires --allow-prerelease for that operation only, not an auto-update opt-in.
Review the publisher before approval; repair does not prove runtime health or
justify bypassing an antivirus detection.
