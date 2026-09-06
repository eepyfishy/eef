# EEF v0.3.2 — UX/UI overhaul

A simpler first installation for EEF and the EEF device app (EEFN), including
both roles on the same PC. See the [getting-started guide](https://github.com/eepyfishy/eef/blob/main/docs/FIRST_RUN.md).

- Graphical setup with a folder picker, startup/launch choices, progress, and
  a completion screen. Per-user installation; no administrator bypass.
- New Home, Devices, Models, Permissions, Network, Settings, and Advanced pages.
- Automatic same-account local pairing, detected hostname, persistent device
  identity, and a single running instance per configuration.
- Configure devices from EEF. Owners approve proposed changes locally unless
  they explicitly grant remote-management permission.
- Restart buttons for both apps and optional restart after saving device
  settings. Installed updates can be applied with Restart.
- Explicit model installation, download progress/cancel, and model selection.
  Existing Ollama is preferred automatically; GGUF models use the included
  llama.cpp runtime. Neither installer contains or downloads a model at setup.
- Friendly permission forms, hardware metadata, live connection/retry/error
  states, redacted diagnostics, and a narrow-screen layout.
- No reserved 5 GB floor; required payload-space checks remain.

Validation: 35 Rust tests passed. Final release binaries passed isolated
same-PC pairing, identity/rename, permission-form, remote-approval/configuration,
restart, and narrow-screen browser tests. Mock Ollama tests covered explicit
install/select/registration and cancellation of a stalled download. A real,
hash-verified Qwen 2.5 0.5B GGUF download ran CPU inference through EEF and the
bundled llama.cpp server, including inference after restarting the selected
model. The test model is not included in either installer.

Both final installers passed native Install/Finish interaction checks,
isolated install/upgrade, startup/config preservation, per-file inventory,
Python media import, and runtime startup checks. Both passed local Defender
scans with real-time protection enabled and definitions `1.459.77.0`.

SHA-256:

```text
eef-installer.exe (17,403,586 bytes)
dd1f4ba70c0cb1c0a8e8ac2eb2395719635ab850488cc4da1c029b50c46cbec1

eefn-installer.exe (109,176,861 bytes)
882c5729de69727ac8a43fed6c588fc028e4cae16db6931b7490e718eb27212d
```

Known limits: LAN connection codes are manual; coordinator priorities are
reconnect ordering, not a shared election. Camera/microphone device selection
uses system defaults, and offline-device history lasts for the EEF session.
Some uncommon settings still use Advanced. Two-PC failover, live media
capture, vision models, and Wake-on-LAN need tests on your actual devices.

These open-source installers remain unsigned. Passing local Defender scans
does not establish Microsoft clearance or browser reputation. The original
v0.3.0 detection remains unresolved; do not restore a quarantined file or add
an antivirus exclusion. See the [antivirus investigation](https://github.com/eepyfishy/eef/blob/main/docs/ANTIVIRUS.md).
