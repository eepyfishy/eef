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

## Optimized-binary validation (2026-09-20)

The version-bumped source passed all 178 locked Rust tests. Optimized alpha.4
binaries passed these isolated suites:

- Request preview: `request-preview-LmvH36`.
- Model transfer commands: `model-downloads-AP1Yvb`.
- Models, jobs, receipts and approval: `model-metadata-9OWTEO`.
- Nonblocking startup, owned process failure and preview: `model-startup-AFPxu8`.
- Node commands: `node-commands-q9D1QZ`.
- Scoped peer discovery: `peer-discovery-GZhwyH`.
- First-run and browser regression: `first-run-3F5Z2n`.

Formatting and backend/browser dependency checks passed. These process suites
use isolated configurations and synthetic model services. The new diagnostic
client also passed five authenticated pings through the existing alpha.2
coordinator to an existing alpha.3 remote node. That is a connectivity and
client-compatibility check, not a remote upgrade or inference acceptance test.

The remote node remains on alpha.3: its stable-only feed rejects prereleases,
and remote process/filesystem deployment is not enabled. An explicit local
installer upgrade is required; permissions and update feeds were not changed.

## Final build evidence

Final packages were built from clean source
`101881c9d79bc22b355da3d748c815a6c524344a`. Subsequent release-tag changes record
validation evidence only; the packaged revision is in `provenance.json`.
All 178 Rust tests also passed with dashboard features disabled after the
version bump. The final embedded Python environment contains 20 distributions;
all 18 applicable dependency requirements resolved within that bundle, and
the media import check passed.

Local Defender scans passed for executables, runtime archives, unpacked final
bundle and both final installers with engine `1.1.26080.3`, definitions
`1.459.290.0`, real-time protection enabled. Both installers are `NotSigned`.
These results are not Microsoft clearance or a second-PC download verdict.

| Installer | Bytes | SHA-256 |
| --- | ---: | --- |
| `eef-installer.exe` | 17,933,275 | `fc52adb9406bf15e230c9c7dbef7d831d655dba6b6fc93581399b2a3f77e14cf` |
| `eefn-installer.exe` | 109,852,658 | `5bbf3269e09a6b6ad58a53b4f8c07be05c02d2fd8c8abd1108e4920b9802222f` |

Final scan reports are retained locally in `.validation/v0.4.0-alpha.4`.
Both final installers passed isolated fresh installation and repeated upgrades,
configuration/startup preservation, explicit startup removal, stale version
selector backup, installed file inventory checks and executable version checks.
Bundled Python imports and the minimal llama.cpp runtime version check passed.
Evidence: `.validation/install-test-944b5e0b0520433694008f59d2543e2b`.
These tests did not alter either existing personal installation.
