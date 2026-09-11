# v0.4.0 implementation and acceptance tracker

## Superseded planning order (2026-09-11)

The active plan is now [Command-first core roadmap](ROADMAP-CORE.md), based on
the owner's post-alpha architecture proposal. Visual UI work is paused; commands,
automation and core services take priority. Future normal PC node setup will
bootstrap a configurable lightweight LM, superseding this tracker's earlier
no-initial-model policy. Published v0.4.0a behavior has not changed.

The milestone table, requirements and dated checkpoints below are retained as
historical evidence, not the current implementation order. See
[release evidence](RELEASE-0.4.0a.md) for the final 84-test alpha validation;
earlier checkpoint counts/version/no-release statements describe their own dates.

Status: the owner approved a reduced **v0.4.0a alpha** release on 2026-09-07.
The full roadmap remains in progress. Existing v0.3.2 artifacts and stable update
feeds are preserved; the alpha is opt-in and does not claim deferred milestones.

Current workspace version: `0.4.0-alpha.1` (GitHub tag/title `v0.4.0a`). Packaging
prereleases requires explicit `-AllowPrerelease`; development identifiers remain
rejected. See [RELEASE-0.4.0a.md](RELEASE-0.4.0a.md) and [STATUS.md](STATUS.md)
for the implemented reliability slice and explicitly outstanding work.

The owner confirmed this order on 2026-09-06. Existing compatible systems are
the baseline; a schema or placeholder is not a completed milestone.

| Priority | Milestone | Acceptance boundary | Status |
| --- | --- | --- | --- |
| 0 | Existing two-PC reliability | Input through the running node; explicit retry/stop behavior; update authorization; consistent storage limits | In progress |
| 1 | Origin, areas, resources, metadata | Immutable request context, hierarchical areas, unique resources, compatible registration; extend scheduling | Core implemented and automated checks passed; physical tests pending |
| 2 | Per-node model management | Normalized capabilities, installed-model browser, owner-approved downloads, progress/cancel/disk checks; Ollama and local GGUF first | Expanded and locally tested; verified Ollama storage and physical acceptance remain |
| 3 | Durable persistent jobs | Persist definitions and lifecycle; job identity separate from assignment; safe restart/migration | One-shot journal and node controls implemented; recurring jobs and controlled migration pending |
| 4 | Camera and audio workflows | Webcam/RTSP resources, authenticated direct bounded streams, lightweight detection, event-triggered VLM, microphone correlation | Pending |
| 5 | Transport and mesh expansion | Reuse framing, link/topology interfaces, separately authenticated peers; ESP32 TCP sensor before ESP-NOW/multi-hop | Pending |
| 6 | Distributed workload optimization | Tracking ownership, parallel vision, measured links, bandwidth/power-aware routing, controlled migration | Pending |
| 7 | Coordinator cluster | Election, replicated critical state, stale-leader fencing; validate together before claiming safe active/standby | Pending |

## Cross-cutting owner requirements

- Say **node** in the user interface. Users interact through EEFN; EEF coordinates.
- Restore the compact navy/blue visual direction. Background updates must not
  disrupt text selection, copying, editing, focus, or expanded details.
- One explicit switch grants a feature's supported operations within the
  current Windows account's rights. No mandatory file/folder/app allowlists.
  Existing scoped configurations must not silently gain unrestricted access.
- Wi-Fi and Bluetooth are opt-in node capabilities, subject to hardware and OS
  support. A permission is not a claim that unsupported hardware works.
- Relay audio and camera/screen feeds over authenticated, bounded data paths,
  independently of heartbeat/control traffic, with visible start/stop state.
- Models are owner-selected, never bundled on first install. Prefer existing
  Ollama, otherwise the included llama.cpp runtime.
- Keep two installers, EEF and EEFN; preserve per-user installation, optional
  startup, restart controls, remote configuration approval, and anonymous
  repository-local contributor identity.
- Investigate unresolved Defender reports. Signing is publisher identity and
  integrity, not proof of malware clearance. Do not weaken Windows protection.
  Public signing may require verified identity and an approved signing provider.

## Release gates

Record automated and physical-device evidence separately. Two-PC reconnect,
permissions and cancellation tests; real camera/microphone/radio tests; stream
limits/authentication; durable job recovery without unsafe duplicate actions;
partition/rejoin and stale-leader tests are required for the corresponding
claims. Unsupported or untested behaviors remain explicitly incomplete.

## Development checkpoint: 2026-09-07

- 52 workspace Rust tests passed on Windows with the locked dependency set.
- `tools/test-dashboard-live.mjs` passed both-role rendering, accessible navigation
  names, unchanged DOM identity and copy-selection preservation using local mocks.
- `tools/test-first-run.mjs` passed with real EEF/EEFN processes and Chrome:
  running-node browser/CLI input, no duplicate registration, scoped-grant migration,
  full HTTP feature grant, selection/caret preservation across polls, persistent
  Stop/Resume, remote approval/configuration, reconnect, model pull/select/cancel
  through a mock Ollama service, retained expanded details, and mobile layout.
  Evidence: `.validation/first-run-0fmzxF/`. This is **one-PC integration**, not
  physical two-PC or real camera/audio/radio acceptance.
- Defender rechecks on this PC passed for both original v0.3.2 installers and
  development debug executables with definitions `1.459.81.0`; the original
  second-PC detections remain unresolved.
- Signing scripts passed PowerShell syntax checks. Real certificate signing,
  timestamping, and signed-installer installation remain untested/unconfigured.
- No new release was published and no update feed changed. No GitHub credentials
  or signing account were activated, and no global contributor settings changed.

Origin/area/resource metadata and context-aware routing are now implemented;
see API.md for exact behavior and limits. The remaining milestones above still
gate the complete v0.4.0 release.

## Origin/resource checkpoint: 2026-09-07

- 60 workspace Rust tests passed with locked dependencies on Windows.
- Added validated immutable context and bounded, backward-compatible registration
  metadata. Wire tests verify forged origin replacement, registration-before-input,
  old-node defaults, area snapshots and removal of private adapter bindings.
- Routing tests cover origin preference, areas, busy/unhealthy nodes, stale versus
  fresh heartbeats, unavailable resources and exact resource constraints. Planner
  tests retain context across capture/inference steps without confusing a physical
  resource ID with the inference model selection.
- Executor tests check node-qualified ownership, capability matching, disabled
  resources, owner binding precedence and unchanged feature permission gates.
- SQLite migration/reopen/retention tests preserve legacy messages and new context.
  Conversation history is bounded; durable job recovery is still pending.
- Both browser suites passed. Real-process evidence is in
  `.validation/first-run-zp1xCk/`: local pairing, permission migration, remote
  configuration, area/resource forms, a bound file request returning through the
  running node, unchanged origin in executor diagnostics and history after EEF
  restart, plus existing copy/focus/model/reconnect regressions. Model downloads
  use a mock service; file execution uses isolated fixtures, not personal files.
- No physical media/radio test or new Defender clearance is claimed. Existing
  v0.3.2 installers and update feeds remain untouched.

Next: finish model-management/storage acceptance gaps, then durable jobs and the
remaining media, mesh, optimization and coordinator-cluster milestones.

## Model/storage checkpoint: 2026-09-07

- 69 Rust tests passed with locked dependencies; both application binaries built.
- On-demand installed-model inspection normalizes reported Ollama capabilities,
  requires owner choice for unknown metadata, and rejects known embedding-only
  models for generation. Vision models support text routing without allowing
  text-only models to satisfy vision requests.
- Bounded metadata/progress parsing, layer-aware counters and a distinct cancelled
  state. Tests cover fragmented/unterminated progress, oversized bodies with and
  without Content-Length, errors, unknown capabilities and stale cancellation IDs.
- GGUF disk checks repeat during download; existing content is hash-verified and
  reused. Small fixture tests verify reuse and preservation of corruption. No
  real model was downloaded during this checkpoint.
- Updater tests cover local and chunked size bounds, exact-size/hash rejection,
  valid staging for both products and refusal to overwrite existing versions.
  Downloads remain memory-buffered up to the documented 512 MiB ceiling.
- Fixed a gap in the prior origin checkpoint: EEF's remote-save allowlist omitted
  metadata. Remote area/resource edits now persist, retain local approval rules,
  and appear in remote status; dedicated tests cover these paths.
- Browser mocks verify auto-detected vision selection, explicit fallback for older
  services and existing copying/editing behavior. Real-process integration with
  mock Ollama passed again in `.validation/first-run-mP7tyo/`, including cancellation,
  metadata inspection and remote area/resource edits.
- The user-requested preview was stopped; its isolated settings were retained.
  No release, update feed, signing configuration or GitHub identity was changed.

Still pending: verified storage reporting from the actual Ollama service, richer
local vision/projector setup, physical two-PC/media checks, durable jobs, and the
subsequent roadmap milestones. The workspace remains `0.4.0-dev.1`, not a full
v0.4.0 release.

## Durable job development: 2026-09-07

- One-shot plans are saved in the existing coordinator SQLite database before
  dispatch. Full UUID job/task identities are independent of recorded node,
  resource and model assignments. Results persist before dependencies advance.
- Restart never automatically replays unfinished jobs. Completed predecessors
  remain completed; uncertain mutations cannot resume. Safe unfinished work can
  resume only through an explicit owner request. This is not exactly-once remote
  execution, a distributed lease, recurring scheduling, or live migration.
- Node Jobs page uses the existing authenticated transport with immutable origin
  ownership checks, status/step/history views and explicit lifecycle controls.
  Pause/Stop describe in-flight behavior; history removal requires confirmation.
- Logical count/payload limits reject writes before dispatch; histories and
  assignment lists are bounded. A full or damaged journal fails closed instead
  of being silently reset. SQLite overhead/free pages are additional disk usage.
- Added tests for durable results/context/assignments, exclusive DB ownership,
  safe recovery, unsafe resume rejection, pause/resume, paused stop, timeout
  dependencies, shutdown, quotas, stale attempts and node ownership isolation.
- Corrected the browser restart check: waiting merely for connected status could
  observe the old instance during the restart delay. It now requires a changed
  runtime instance ID, node reconnection, and identical completed job history.

The preview remains stopped. Published installers, update feeds, signing setup,
and GitHub/global contributor settings are unchanged.

Validation: **80 workspace Rust tests passed** with locked dependencies; both
debug applications built. Both browser suites passed. Real-process evidence is
in `.validation/first-run-SPJdR4/`, including the node Jobs page, copy/expanded
history preservation, unchanged completed jobs after a verified new EEF instance,
and refusal to read/remove coordinator-origin jobs from an executing node.
These are isolated one-PC fixtures, not physical two-PC or media acceptance.

### Recovery/storage follow-up

- Reserved logical quota headroom for active and interrupted jobs. A lowered or
  full logical budget no longer blocks safe recovery, Stop, or finished-history
  removal. Physical disk exhaustion/corruption can still block SQLite writes.
- Additive reservation migration also handles a process stopping after the
  schema change but before data backfill. New work never replays automatically.
- Added Settings forms for job count and MiB limits and Jobs usage reporting.
  Saved EEF preferences survive a page reload before restart; status continues
  to show applied limits. Redacted keys preserve saved rather than stale keys.
- **83 Rust tests passed** with locked dependencies. Browser mocks cover job
  Resume/Pause/Stop/removal controls; real-process validation passed in
  `.validation/first-run-rMIdzj/`, including storage forms, saved/applied separation,
  reload, a second verified EEF restart, and node-visible effective usage.
