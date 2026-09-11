# Architecture conformance status

## v0.4.0a2 diagnostic alpha

Workspace version `0.4.0-alpha.2` packages the command/service and scoped-discovery
increments below, plus minimal local diagnostics, authenticated ping/latency
probes and explicit-address private pairing codes. See
[release scope](RELEASE-0.4.0a2.md) and [two-PC testing](TWO_PC_TEST.md).
98 Rust tests and final packaged command/discovery/first-run/browser and installer
validation passed; hashes and scan details are in the release notes. Physical second-PC acceptance follows
installation; MSI, default LM, independent peer credentials and leases are not
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
