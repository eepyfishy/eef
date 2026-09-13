# Architecture conformance status

## v0.4.0a3 release preparation

This opt-in alpha packages the backend checkpoints below. Stable update feeds
remain unchanged. Both roles now reject prerelease feed updates at check and
installation time; a changed manifest cannot bypass the installation guard.
See [release scope and limits](RELEASE-0.4.0a3.md).

## Unreleased optional runtime startup isolation

- Failed llama.cpp startup now cleans up the local model group and continues
  node registration without advertising that group. Missing Python or failed
  plugin inspection withholds affected capabilities rather than disabling the
  core connection. Owner settings remain unchanged; no silent model substitution.
- Status and diagnostics expose bounded `startup_issues` codes, without raw
  exception text or paths. This is a startup snapshot, not continuous health.
  Nonblocking model loading and individual slot recovery remain future work.
- All 153 Rust tests passed both with and without dashboard features on
  2026-09-13. Isolated `.validation/model-metadata-rIcvW6` verified connected job
  commands and system ping despite missing model/Python runtimes, plus approved
  remote restart and recovery. It also passed receipt-loss and authorization
  revocation checks. No physical model/hardware test or deployment is implied.
- Command/discovery, first-run and browser-copy regressions passed:
  `.validation/node-commands-oRA9UM`, `.validation/peer-discovery-UfZBLg`,
  `.validation/first-run-Zbjo2h`. No model download or installed-node change.

## Unreleased job receipt recovery

- The node CLI generates an operation receipt before sending. Updated EEF stores
  it in the authenticated job origin context while keeping transport reply IDs
  independent. `eefn jobs find --operation-id UUID` searches only that origin's
  retained creation history, filtering before the bounded result page.
- Lookup does not replay work or deduplicate submissions; an empty match is not
  proof that an uncertain request was never accepted. It is not a control-command
  receipt ledger. See [command recovery semantics](COMMANDS.md).
- All 152 Rust tests passed on 2026-09-13, including persistence/reopen, filtering
  beyond the first 100 jobs, origin scoping and independently correlated replies.
  Isolated `.validation/model-metadata-bnagjD` deliberately dropped the local
  creation reply, verified exactly one submission and recovered the saved job
  using the CLI receipt. It also passed the earlier model/auth/job/restart checks.
- A read-only installed-coordinator check still showed two connected nodes,
  two registered models and no pending restart. Those v0.4.0a2 installations were
  not updated; new-code execution used a fake backend, not physical inference.

## Unreleased remote authorization race fixes

- Legacy remote configuration writes now read, authorize and save under the
  same lock as local commands. They preserve concurrent owner changes and
  cannot restore revoked management approval. Denied writes remain proposals.
- Remote model installs recheck approval at download admission after backend
  inspection, and reject changed model configuration. Approval revocation does
  not retroactively cancel already admitted work; explicit cancellation remains
  available. No new model source or automatic download was added.
- All 150 Rust tests passed on 2026-09-13. The isolated process regression
  `.validation/model-metadata-E9N5Nq` revoked permission during fake backend
  discovery and verified that no pull started, then checked denied remote saves.
  No model weights, installed apps, physical node settings or releases changed.

## Unreleased backend/browser separation checkpoint

- Browser assets are optional in both crates; both executables support `--no-ui`.
  Node HTTP routes moved to `api.rs`, with core status/configuration, connection,
  proposal, startup and update operations owned by `NodeService`. Browser files
  contain presentation routes/native picker only. A source dependency guard
  checks that runtime modules do not import the browser/HTTP adapters.
- Added `eefn connection show/pause/resume/pair-local`. Pausing the outgoing EEF
  connection does not disable the local command API. Connection config edits
  serialize with model edits, preserving unrelated saved settings.
- Saved `dashboard.ui_enabled` / `web.ui_enabled` preferences hide just the
  browser, without changing legacy listener enablement. The node adapter can
  change its browser gate without rebinding its command listener. CLI `--no-ui`
  remains an override; no-dashboard builds cannot enable browser assets.
- 148 default-feature Rust tests passed on 2026-09-12, including browser-gate
  and configuration validation. Saved-preference integration passed in
  `.validation/model-metadata-S3J9GL`: both roles' commands worked with UI hidden,
  node UI enable/disable preserved the command endpoint, and connection controls,
  models, jobs and restarts passed against an isolated fake HTTP backend.
- The earlier no-dashboard build passed 146 Rust tests and process integration
  (`.validation/model-metadata-2i2Vce`); all 148 tests also passed without dashboard
  features on 2026-09-13 after the saved-preference follow-up. This is not real inference or physical
  two-PC acceptance. No installed app, model, release feed or signing status was
  changed. See [boundary and remaining scope](BACKEND-BOUNDARY.md).
- Follow-up command/discovery, first-run/job and browser-copy regressions passed
  on 2026-09-13 (`.validation/node-commands-6qd4MR`,
  `.validation/peer-discovery-yOxtQ3`, `.validation/first-run-RPsvjj`).

## Unreleased node-origin job commands

- Added `eefn jobs list/get/output/pause/resume/stop/remove/generate-text` and
  guarded typed `/api/commands/jobs`. The runtime service uses the running node's
  existing submission connection and actual API discovery; no second node,
  browser controller, local LM or remote-management permission is required.
- EEF preserves authenticated origin ownership and existing durable lifecycle
  rules. Creation acknowledges a job, not completed work; pause/stop report
  intermediate states. Commands distinguish known-not-sent from rejected or
  uncertain mutations and never automatically resubmit after a lost reply.
- Explicit output inspection returns bounded text-model content only, while
  ordinary status remains redacted. It uses the same origin check as controls;
  it does not export file contents, media, parameters or another origin's results.
- 144 Rust tests passed on 2026-09-12. Isolated integration evidence:
  `.validation/model-metadata-YCa5L1` verified origin scoping, text output,
  pause/resume without repeated execution, stop and finished-history removal,
  alongside remote model selection and restart tests. The backend was a fake
  local HTTP service, not real inference or a physical two-PC test.
- Command and discovery regressions passed (`.validation/node-commands-9NIgvh`,
  `.validation/peer-discovery-ltgV03`), as did first-run/job persistence and
  browser-copy regressions (`.validation/first-run-cZdAMC`). No installed app, release artifact or
  model was changed or downloaded. UI development remains paused.

## Unreleased coordinator-to-node model commands

- EEF now exposes `node models` using the same typed model-selection operations
  and CLI adapter as EEFN. Mutations require node-local management approval,
  rechecked under the config lock. Inspection remains read-only. No implicit
  downloads, loads, restarts or legacy full-config save fallback.
- Requests are sent once; uncertain replies are explicitly unconfirmed. Remote
  GGUF/projector paths are validated on the target node, not the coordinator.
- 140 Rust tests passed on 2026-09-12. Isolated integration evidence:
  `.validation/model-metadata-FyzHyP`; old-node refusal/compatibility:
  `.validation/model-metadata-r6gwng`. Command, discovery, first-run/job and
  browser-copy regressions passed (`.validation/node-commands-gHb7T0`,
  `.validation/peer-discovery-lez654`, `.validation/first-run-6iYR9q`).
- A read-only check of the installed coordinator still reported two connected
  nodes and no pending restart. These installed v0.4.0a2 apps were not changed;
  the new feature tests used isolated processes and a fake backend, not real
  inference. No release or automatic telemetry upload was performed.

## Unreleased explicit model-role routing checkpoint

- Added bounded, read-only `eef models route` and its guarded API. It previews
  connected candidates by capability/role/node/backend/model without inference,
  permission changes, readiness claims or reservations.
- Existing routing enforces explicit `model_role` and backend constraints even
  during tier fallback. Existing durable jobs support `generate_text` and
  `analyze_image`; automatic interpretation/planning remains pending.
- 135 Rust tests passed on 2026-09-12. Isolated integration evidence:
  `.validation/model-metadata-y8lWNI`. A fake backend verified exact role dispatch,
  missing-role job failure without fallback, and no inference during preview.
  This is not real inference or a physical two-PC test. Installed apps and
  published releases remain unchanged.

## Unreleased local restart and API discovery checkpoint

- Added `eefn restart` with local-owner node/runtime preconditions, one send,
  bounded completion observation and explicit uncertain outcomes. It shares the
  node core restart operation with existing browser/approved remote controls.
  Completion is a new initialized runtime, not a workload/model/reconnection claim.
- A bounded per-config local API marker records the actual bound endpoint.
  Network/model/input commands and restart polling follow it instead of pending
  port settings. It is identity-checked and loopback-only, with old-node fallback
  only when the marker is absent. No remote listener was added.
- 133 Rust tests passed on 2026-09-12. Isolated process testing verified restart
  and API port migration (`.validation/model-metadata-YyzqX8`); command and
  discovery tests passed (`.validation/node-commands-g9t1Vl`,
  `.validation/peer-discovery-QXwoD4`), as did first-run/job/browser regressions
  (`.validation/first-run-ezjgWB`). No real model download or release deployment.

## Unreleased node model-selection commands

- Added shared node `models show/select-ollama/select-gguf/hints/provider/remove`
  commands and the guarded local owner API. Offline commands use the instance lock;
  running commands use the existing node service and configuration lock. No JSON
  editing, model download or automatic restart is required to save a selection.
- Extended existing backend selections with optional capability restrictions and
  role labels, projected into metadata and enforced by updated node executors.
  Unsupported capabilities are rejected; an empty capability list disables that
  model's inference operations. Role-based scheduling is still pending.
- Saved selections and registered models remain distinct. No-op edits avoid writes;
  corrupt/missing config, wrong targets and invalid hints are rejected. Removal
  affects selection only, not model files. GGUF ports can be allocated internally.
- 130 Rust tests passed on 2026-09-12. Evidence:
  `.validation/model-metadata-3wV2vO`, `.validation/node-commands-YJoF6P`,
  `.validation/first-run-lVU6xZ`, `.validation/peer-discovery-sDhh29`;
  browser copying/focus checks also passed. Tests use isolated configs and a fake
  backend, not real inference. Published apps/feeds and the real network were not
  changed. See [commands and downgrade caveat](COMMANDS.md).

## Unreleased versioned model metadata checkpoint

- Added a shared, bounded `model_metadata` registration schema: multiple
  capabilities/input/output modalities, optional roles, lifecycle, availability
  and resource estimates. New Ollama/GGUF advertisements preserve legacy fields.
  Unknown values remain unknown; model names never infer capabilities.
- Registration validates the entire model snapshot before replacing it, including
  empty snapshots. Other nodes and load metrics are preserved. Duplicate identities
  and malformed/unsupported metadata are rejected, not silently downgraded.
- Inventory and scoped peer discovery carry validated metadata. Existing text/vision
  routing filters declared capabilities, input/output support and unavailable/error/
  loading state. Backend-specific failures no longer poison another backend's model;
  updated nodes enforce an explicitly requested backend without silent fallback.
- General owner configuration for non-text/vision models, role-based planning,
  default LM, resource-aware admission, lifecycle controllers and leases remain
  pending. See [contract and compatibility](MODEL-METADATA.md). No UI changes.
- 124 Rust tests passed. Isolated fake-backend protocol tests passed with both
  development binaries (`.validation/model-metadata-wnlYlL`) and the published older
  node (`.validation/model-metadata-G5SVAH`), using separate temporary configs.
  They verify registration, CLI/inventory, stable identity, model-list clearing,
  and new-node backend enforcement. These are not real inference or two-PC tests.
- Command integration (`.validation/node-commands-uOMOCv`) and scoped discovery
  (`.validation/peer-discovery-rxQzSI`), first-run/job persistence
  (`.validation/first-run-OQ15vZ`), and dashboard copying/focus regressions passed.
  Validation recorded on 2026-09-12. Published
  installers/feeds and both real installed apps remain unchanged; no model was
  downloaded. No signing or Microsoft clearance is claimed for this checkpoint.
  Both real nodes remained connected; installed executable hashes were unchanged.

## Unreleased model-inventory and approved restart checkpoint

- After the owner enabled local remote management, the physical second-PC node
  acknowledged restart and reconnected under the same stable ID with a new runtime
  ID. Evidence: `.validation/live-discovery-QGI8x0/results.json`. This closes the
  positive remote-restart gate left pending in the earlier checkpoint below.
- Added `eef models list` and guarded GET `/api/commands/models`: bounded pages of
  registered models, node/capability filters, distinct node/backend/model identity,
  and registration freshness. Unknown roles, lifecycle and resource estimates are
  null; paths, addresses and secrets are omitted. No backend scan/load/download.
- This is a read-only legacy normalization slice. General multi-capability model
  configuration and registration, role selection, and scheduling changes remain
  pending; published v0.4.0a2 nodes remain compatible. No visual UI work.
- 113 Rust tests passed, including model normalization, redaction, empty inventory,
  identity conflicts, paging and response limits. Command, peer-discovery,
  first-run and browser copying/focus regression checks passed.

Final inventory checkpoint evidence (2026-09-12 local time):
`.validation/node-commands-SzT9ir`, `.validation/peer-discovery-vsS9dj`,
`.validation/first-run-VTll5F`, and
`.validation/live-discovery-3S9iBX/results.json`. The live run used two physical
PCs and published v0.4.0a2 nodes. Inventory reported two advertised models on the
local node and an empty model list on the connected second-PC node. Discovery
grant/revoke, two coordinator restarts, and approved remote node restart passed;
all ten remote pings passed at 2.82-6.14 ms. No inference or model download ran.
The development coordinator SHA-256 was
`18a9a4189cc0711a5a60296448325e427ff398b9bc245baac500f96e9548aefc`.
Its local Defender scan passed with engine `1.1.26080.3`, definitions
`1.459.158.0`; this is not Microsoft clearance or a signing claim.
The original discovery policy and published coordinator executable were restored
(hash verified); both nodes reconnected with no pending coordinator restart.
No release assets, feeds or remote node binaries were changed.

## Unreleased command/restart follow-up

- Added coordinator-owner `discovery show/grant/revoke` and `restart` commands,
  backed by shared core services and guarded local APIs. Exact, directional
  disclosure only; saved/applied policy and restart completion remain distinct.
- Serialized saved-config edits preserve unrelated pending settings, reject
  corrupt/missing configuration and keep no-op grants from rewriting files.
- Added `eef node restart --node ID`, using the existing node-owner management
  approval. Bounded completion checks require the same stable node ID and a new
  runtime ID. Lost replies/timeouts are uncertain, not automatic restart retries.
- Bounded outbound request queue waits and cleaned up cancelled response waits.
  These changes are in the coordinator's shared transport; no new wire protocol.
- 109 Rust tests passed. Physical testing confirmed real two-node
  registration, grant/revoke behavior, reconnect after two coordinator restarts,
  and 10/10 remote pings. The remote node's restart permission was off: denial
  passed; positive physical remote restart was confirmed in the subsequent checkpoint above.
- Published v0.4.0a2 installers/feeds are unchanged. No UI redesign, permission
  widening, model downloads or signing claim. See [commands](COMMANDS.md).

Final follow-up evidence (2026-09-11): `.validation/peer-discovery-21turW`,
`.validation/node-commands-BUeKRY`, `.validation/first-run-LrJf2O`, and
`.validation/live-discovery-wIG53K/results.json`. The live run used two physical
PCs with published v0.4.0a2 nodes and a locally built development coordinator
(SHA-256 `8fadfbfd78a64be8e3f6d8658ed4b7a8aa9ac909abf4217919cb51abdd379eda`).
Its local Defender scan passed with engine `1.1.26080.3`, definitions
`1.459.158.0`; no Microsoft clearance is implied. The final ten remote ping
samples passed at 3.00-5.78 ms. Reconnect/grant/revoke and denied remote restart
passed; no actual second-PC node restart was sent without local approval.
After restoration, both nodes reconnected, no pending restart remained, the
original self-only discovery policy was applied, and the published executable's
hash matched its backup. No release assets or test network secrets were published.

## v0.4.0a2 diagnostic alpha

Workspace version `0.4.0-alpha.2` packages the command/service and scoped-discovery
increments below, plus minimal local diagnostics, authenticated ping/latency
probes and explicit-address private pairing codes. See
[release scope](RELEASE-0.4.0a2.md) and [two-PC testing](TWO_PC_TEST.md).
98 Rust tests and final packaged command/discovery/first-run/browser and installer
validation passed; hashes and scan details are in the release notes. Physical
connectivity was subsequently confirmed; full workload acceptance remains pending.
MSI, default LM, independent peer credentials and leases are not
claimed by this test release. Diagnostic reports are local, not automatic uploads.

## Active direction (2026-09-11)

[Command-first core roadmap](ROADMAP-CORE.md) supersedes the original milestone
order. UI polish is paused. Shared command services, identity/address separation,
coordinator advertisements, general model lists, a default lightweight node LM,
structured interpretation, authorized workloads and local/peer execution are
the next increments. Most are still planned extensions, not completed features.
The published v0.4.0a implementation below remains the baseline; its installers
still do not supply/download a model.

Implementation packaged in v0.4.0a2: shared `NodeService` state/model operations, network
show/set commands, API-only `--no-ui`, validated independent node/coordinator
advertisements and EEF node-view propagation. 91 Rust tests and command/browser
integration passed; see [commands and limitations](COMMANDS.md). Peer-address
distribution is now implemented as exact owner-scoped pull discovery with
monotonic freshness and bounded sanitized pages (`network peers`). Discovery
does not authorize direct communication. Leases, general model metadata,
default LM and MSI remain pending. The discovery follow-up passed 97 Rust tests
and a two-logical-node command integration test; physical acceptance is pending.

## v0.4.0a alpha scope

The earlier workspace was `0.4.0-alpha.1`, presented as **0.4.0a**. The owner approved
shipping this reduced alpha scope and deferring the remaining features.
See [RELEASE-0.4.0a.md](RELEASE-0.4.0a.md) and [ROADMAP-0.4.0.md](ROADMAP-0.4.0.md)
for the historical prerequisites and seven milestones. They are not all
implemented; development now covers the reliability prerequisite and an
origin/area/resource-routing, model management, and durable one-shot job
implementations with automated acceptance checks.

- Node-first browser and CLI input over the running node's authenticated
  connection; bounded pending requests, no submission replay after disconnect.
- Encrypted EEF reachability probe before authentication/registration, without
  creating a node record. Older EEF's encrypted auth denial is accepted only as
  service reachability; a fresh socket still requires normal authentication.
- Persistent Stop/Resume connection controls. Stop waiting for a reply is
  explicitly distinct from cancelling work already running on other nodes.
- Mutating/unknown tasks no longer retry automatically unless their caller
  explicitly sets `constraints.retry_safe`; read-only retry behavior remains.
- Feature-wide permission switches, retaining default denial and old scoped
  configurations. Remote update installation requires its own permission and
  cannot replace the locally selected update feed.
- Compact navy/blue dashboard, node terminology, stable DOM updates that preserve
  selection and editing, and decorative icons excluded from accessible names.
- Bounded inbound frame reads, random authentication nonces, atomic live identity
  reservation, and response matching against the authenticated executor.
- Update extraction checks unpacked size against free space, stages new files,
  and refuses to delete an already installed version. No fixed 5 GB reserve.
- Optional signing build hooks and certificate-aware payload framing. No public
  certificate has been configured and no signed release has been produced.
- Authenticated submission-origin snapshots retained through goals, tasks,
  inference transport, events, replies and bounded SQLite conversation history.
  Compatible metadata registration and additive history migration.
- Hierarchical node/resource areas, node-qualified stable resource IDs, owner
  adapter bindings and locality-aware routing after suitability/freshness checks.
  Explicit unavailable resources fail without falling back to another resource.
- Node location/resource forms, also available through EEF remote configuration.
  Resource creation is independent of permission grants and hardware activation.
- Remote-save now explicitly includes validated metadata; remote status returns
  applied areas/resources and last-request diagnostics. Tests cover both proposal
  approval and trusted remote management without changing identity or grants.
- Model selection inspects Ollama's reported capabilities on demand. Unknown
  capabilities require an explicit owner choice; embedding-only models are not
  offered as text generation. Selected vision models can also serve text requests.
- Model-service metadata is bounded to 4 MiB; progress lines to 64 KiB and 4096
  layers. Progress retains layer counts during verification. Cancellation has its
  own terminal state and optional job-ID matching; it is not an installation error.
- GGUF downloads recheck remaining disk space during transfer, verify existing
  files before reuse, and preserve damaged files/indexes instead of overwriting
  them. The UI distinguishes node-drive free space from unknown Ollama storage.
- Update manifests are capped at 1 MiB and artifacts at 512 MiB, with optional
  exact `size_bytes`, hexadecimal checksums, and streamed bounds even without
  Content-Length. Artifacts are still buffered in memory within that limit.

- SQLite-backed one-shot job definitions, attempts, results, origin and executor
  assignments. Checkpoints precede dispatch and dependent steps. A local database
  ownership lock prevents two coordinators opening the same job journal.
- Interrupted jobs do not execute on startup. Explicit resume retains completed
  steps and refuses uncertain non-idempotent actions. Pause/Stop finish the current
  claimed batch; they do not acknowledge remote cancellation or undo effects.
- Jobs history, step details and Pause/Resume/Stop controls in the node app over
  its existing authenticated connection. Nodes see/control only their own origin
  jobs; the loopback EEF owner API can manage all jobs. Finished history removal
  is explicit. UI responses omit raw parameters, file contents and media results.
- Shutdown joins node submissions and interrupts active runners before reopening
  the journal. Cancellation drops scheduler capacity guards. Runtime instance IDs
  let integration checks distinguish the old instance from a completed restart.
- State-dependent recovery headroom within the logical job budget keeps safe
  interruption/stop/removal available even after lowering the quota. Older
  development journals migrate additively, including partial migration recovery.
  Dashboard forms configure history count and MiB budget; Jobs shows effective
  network usage. Physical disk exhaustion and corruption still fail closed.
- EEF configuration forms load saved preferences, not stale running settings,
  while status keeps reporting applied settings until restart. Subsequent saves
  preserve a redacted saved network key rather than reverting to the running key.

Known remaining boundaries: real two-PC acceptance, remote-task cancellation
acknowledgements, verified Ollama-server storage reporting,
physical resource acceptance, recurring jobs and controlled migration,
media relay/correlation, Wi-Fi and
Bluetooth controls, mesh/ESP-NOW, distributed optimization, and coordinator
consensus/replication/fencing are not completed by this slice. Defender reports
against v0.3.2 remain unresolved; passing development tests do not clear them.

This file distinguishes working behavior from planned architecture. It prevents
the open-source project from presenting an unsafe approximation as complete.

## UX/UI changes in 0.3.2

- Graphical per-user installer with folder picker, startup/launch choices,
  extraction progress, and a completion screen. No administrator bypass.
- Home, Devices, Models, Permissions, Network, Settings, and Advanced pages;
  form-based common settings, explicit save/apply/restart, and mobile layout.
- Persistent generated device ID, detected hostname, single-instance launch,
  and same-account local EEF pairing in either app launch order.
- Live connection/retry/error states and applied permissions; Windows hardware
  metadata is detected without opening the camera or recording audio.
- Owner-requested model downloads, progress/cancel, hash-verified GGUF catalog,
  and Ollama-first selection. No model is included or downloaded on installation.
- Device configuration through EEF's authenticated node transport. Owners
  approve proposals locally unless they explicitly allow remote management;
  remote requests cannot grant themselves that permission or replace device ID.
- Restart controls for both roles; optional automatic restart after saving
  device settings. An installed update is handed off when Restart is used.
- Local dashboards reject cross-site browser requests and non-loopback Host
  headers. They are local controls, not remotely exposed management servers.
- The old fixed 5 GB floor is removed. Payload space checks remain.

This is not completion of the entire proposed UX/architecture roadmap.
Automatic LAN discovery, per-camera/microphone selection, richer voice/provider
forms, persistent offline-device history, and a larger curated model catalog
remain future work. Offline history currently lasts for the EEF session.
Detection is not proof a media permission or driver works; live recording,
vision inference, Wake-on-LAN, and two-PC failover require device-specific tests.

## Maintenance changes in 0.3.1

- Hash-locked Windows Python runtime dependencies and retained vendor checksums.
- Component and installer Defender scan gates, dependency provenance, and
  per-file bundle inventories; see [ANTIVIRUS.md](ANTIVIRUS.md) for the
  original detection and the limits of the new passing scans.
- Fewer bundled llama.cpp executables, explicit installer launch consent,
  quiet-upgrade startup preservation, and clearer Windows version metadata.
- Isolated installation/upgrade regression coverage for both installers.

This maintenance release does not implement the proposed 0.4 architecture
roadmap or establish that the 0.3.0 detection was a false positive.

## Implemented in 0.3.0

- Rust coordinator and PC node executables in one workspace.
- Normal `node -> EEF -> selected node(s) -> EEF -> originating node` requests.
- Authenticated, encrypted node transport with configurable endpoints and
  priority-ordered reconnect/failover across the configured endpoint list.
- Live capability/model registration and load, latency, capacity, hardware,
  permission, modality, and constraint-aware routing.
- Dependency-aware task plans, concurrent ready subtasks, timeouts, retries,
  cancellation state, structured results, and node reassignment on retry.
- Structured identity, mutable, working, event, conversation, and task state.
- Default-deny node policies for scoped filesystem access, exact allowlisted
  process execution, application control, host/method-limited HTTP, configured
  STT providers, Wake-on-LAN, microphone, audio, TTS, camera, screen, and input
  control.
- Filesystem roots are canonicalized and junction/symlink escapes are rejected;
  public HTTP destinations are DNS-checked and pinned with redirects disabled.
- Owner-selected Ollama and llama.cpp models; no coordinator model and no model
  in the node bundle.
- Optional isolated Python plugin sidecars using the bundled CPython runtime.
- Configurable `off`, `prompt`, and `auto` verified updates for both programs,
  with version handoff and rollback.
- Local EEF and EEFN dashboards for full configuration, capability permission
  switches, and optional startup.
- Two self-extracting Windows installers with per-user defaults, config
  preservation, generated coordinator secret, and a 5 GB storage floor.
- Standalone EEFN operation with zero coordinators; EEF only becomes
  operational when at least one node connects.

## Not yet complete

- Automatic coordinator discovery, quorum election, replicated coordinator
  state, and partition-safe split-brain prevention. Configured node priorities
  already provide deterministic failover when the preferred EEF disconnects.
- A separately authenticated EEFN peer mesh that remains online when every EEF
  coordinator is unavailable.
- Transport plugins beyond TCP. The protocol is not coupled to a VPN vendor,
  but the pluggable transport interface is still future work.
- Recurring jobs, data dependencies for ongoing media workflows, remote execution
  fencing/deduplication, and controlled migration. One-shot interrupted graphs
  are retained locally and require explicit safe resume.
- Signed release manifests. Updates currently require HTTPS plus an exact
  artifact SHA-256.

Reliable coordinator election cannot guarantee both availability and a single
leader during arbitrary network partitions without quorum. The intended next
milestone is a fixed-membership, majority-lease coordinator cluster with a
durable term/vote log, followed by state replication. The mesh is a separate
node role and must not silently promote ordinary nodes into coordinators.
