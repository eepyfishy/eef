# EEF v0.4.0a4 - model lifecycle and request-preview alpha

Internal version: `0.4.0-alpha.4`. Opt-in Windows x86-64 prerelease, not the
completed v0.4.0 roadmap. Two EXE installers, one each for EEF and EEFN.

## Added since v0.4.0a3

- Model startup no longer blocks deterministic node commands or registration.
  Failed or exited owned GGUF processes withdraw model availability; explicit
  restart is required. This is process monitoring, not a complete model lifecycle.
- Command-based installed-model browsing, read-only install planning, explicit
  downloads, progress and exact cancellation. Disk and artifact checks apply;
  downloads do not implicitly select or load a model.
- Bounded inference responses reject malformed, oversized or stalled replies
  and preserve completion evidence without automatic retry or fallback.
- `eefn requests preview --text TEXT --json` uses one owner-selected applied
  `request_interpreter` model. It validates an untrusted proposal without
  dispatching work, creating jobs or granting execution authority. Missing roles,
  incomplete responses and restart-invalidated results fail explicitly.
- Preview works without the dashboard or a coordinator connection. Selection,
  downloads and core services remain command-first; no visual UI overhaul.

No model is bundled or automatically installed. CPU candidate evaluation did
not approve a default interpreter. Automatic interpretation, clarification and
approval workflows, execution grants, local pipelines, direct peer media,
continuous listening, coordinator clustering and MSI remain future work.
See [commands](COMMANDS.md), [interpretation limits](INTERPRETATION.md),
[model evaluation](MODEL-CANDIDATE-EVALUATION.md) and [roadmap](ROADMAP-CORE.md).

## Installation and update policy

Automatic updates remain stable-only; stable feeds remain at v0.3.2. Alpha.3
and later reject prerelease feeds at both check and apply time. Install this
alpha explicitly with its installer; do not change feeds to bypass that guard.
Close the app and select its existing dedicated installation directory. Back
up configuration/data first. Manual upgrades preserve configuration, stable
node identity, data and existing startup choices; database downgrade is untested.

Unsigned: no code-signing credential is configured. Historical second-PC
Defender detections remain unresolved. Local scan success is not Microsoft
clearance. Do not disable protection, add exclusions or restore detected files.
No private test addresses, credentials or automatic analytics are embedded.

## Validation

The development checkpoint passed 178 Rust tests with and without dashboard
support plus isolated request-preview, model-startup, model-metadata and
model-download process suites. Their inference backends were synthetic, not
physical two-PC inference or real interpreter-quality acceptance. Release
packaging, installer validation and deployment evidence are recorded separately
when complete; no successful remote upgrade is implied by publishing this tag.
