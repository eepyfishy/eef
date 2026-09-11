EEF coordinator for Windows

v0.4.0a2 diagnostic alpha. TWO_PC_TEST.md describes private-network pairing and
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
Stable update feeds remain on the stable release; alpha testing is opt-in.
