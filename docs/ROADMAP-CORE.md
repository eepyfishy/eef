# Command-first core roadmap

Owner direction: 2026-09-11. Baseline: published `v0.4.0a`
(`0.4.0-alpha.1`). This is the active roadmap, superseding the sequencing and
future model-install policy in [the original tracker](ROADMAP-0.4.0.md).
It distinguishes planned scope from the development checkpoints below.
Keep the working Rust workspace; extend it incrementally, with migrations and
tests. No rewrite, new release, or model download is implied by this document.

The subsequent owner-requested two-PC test release is tracked separately in
[v0.4.0a2](RELEASE-0.4.0a2.md), internal version `0.4.0-alpha.2`. It packages the
completed increments and explicit diagnostics, not the full roadmap.

## Direction and policy changes

EEF is the control plane: it validates requests, plans, authorizes, and observes
work. EEFN is the node runtime: it interprets natural language when available,
enforces permissions, executes authorized workloads, and reports results.
Authorized nodes may exchange data directly; EEF need not relay every payload.

Pause visual UI development. Prioritize reliable commands, diagnostics,
automation, networking, execution, and model handling. UI changes are limited
to exposing/testing core features, truthful status, security, and accessibility
fixes. Preserve the existing compact UI and copying/editing regressions.

Normal future PC node setup will supply or automatically obtain a replaceable,
CPU-capable lightweight LM. This **supersedes the earlier requirement that new
nodes never obtain a model**. Published v0.4.0a installers still contain/download
no model; do not retroactively change their documentation or artifacts.
EEF itself still needs no local LM. GPU/CUDA is optional, never a core dependency.

## Commands own behavior; UI is a client

```text
CLI / automation / dashboard -> shared typed commands -> policy + core services
natural language -> node interpreter -> validated request -> EEF plan + grant
                                                          -> node execution
authorized node <---------------- bounded data ----------------> authorized node
                          status/events/results -> EEF -> origin node
```

Commands are structured application operations, not arbitrary shell strings.
Natural language is an optional input adapter, not required for deterministic
commands. Both paths pass the same authorization and validation boundaries.

- Put configuration validation, model operations, connection control, job
  lifecycle, permissions and listening state in reusable Rust services. CLI,
  HTTP and UI call those services; no business rules in browser code or a
  dashboard-owned object required to operate a node.
- Keep local command/IPC services available without serving the browser UI.
  Discover the running instance through existing per-user discovery; ordinary
  commands must not need a port number or create a second node connection.
- Provide readable help/output and versioned JSON output, stable error codes,
  exit codes, correlation/operation IDs and status/event inspection. Keep logs
  separate from machine-readable stdout. Test Windows console and piped output.
- Long operations return durable IDs where applicable; command timeout or a
  closed UI does not imply remote cancellation. Report requested, acknowledged,
  stopping and stopped states distinctly. Poll/watch reconnect does not resubmit.
- Mutations require the existing local owner or approved remote authority.
  Scripts fail with `approval_required` rather than hanging for a hidden prompt.
  Reusable approvals are explicit and bounded; no blanket approval implied by
  installing the app, selecting a model, or writing an automation.
- Retain command identity/deduplication in the existing durable job journal for
  workload submissions. Do not promise exactly-once side effects. Configuration
  commands retain saved/applied separation and atomic validation.
- Initial command families: status/diagnose, connection, identity/address,
  network inspection, models, requests, jobs, permissions, listening, restart
  and updates. This names planned interfaces, not new executable syntax.

## Mapping onto v0.4.0a

Inspected source, not just the earlier proposal:

| Concept | Existing implementation | Required extension |
| --- | --- | --- |
| Identity and configuration | `eefn/src/setup.rs`, `main.rs::FileConfig`, `client.rs::NodeClientConfig` | Preserve persisted IDs/name; add separately validated advertised endpoints and config migration. |
| Discovery and node view | `eefn/src/protocol.rs`, `server.rs::ConnectionInfo`, `client.rs`; `eef/src/runtime.rs`, `world.rs` | Current socket source address is diagnostic, not a peer listener address. Add explicit registrations, freshness and authorized lookup. |
| Origin/areas/resources | `eefn/src/context.rs::RequestContext`, `NodeMetadata`, `Resource` | Preserve server-stamped origin and private resource bindings; new hints never replace them. |
| Commands/input | `eefn/src/main.rs::Args`, `submission.rs::SubmissionMailbox`, `dashboard.rs`; `eef/src/api.rs` | Existing `--ask` and bounded authenticated submission are reusable. Extract service state and operations from HTTP/dashboard ownership. |
| Model inventory | `eefn/src/client.rs::SelectedModel`, `model_manager.rs`, `model_server.rs::ModelSlot`; `eef/src/model.rs::ModelSpec`, `ModelRegistry` | Lists already exist; generalize the text/VLM modality schema into multiple capabilities/modalities/roles, lifecycle and unknown resource metadata. |
| Planning and durable jobs | `eef/src/task.rs::Goal`, `Plan`, `Dispatcher`, `TaskEngine`; `jobs.rs::JobStore`, `AttemptJournal` | Extend existing identities, attempts and journal for grants, executor acknowledgements and continuous work. No parallel job framework. |
| Enforcement and adapters | `eefn/src/engine.rs::NodeEngine`, `NodePolicy`, `python.rs` | Check each granted operation locally; add bounded local runners and separately authorized peer sessions. |
| Model process lifetime | `eefn/src/model_server.rs::ModelServer`; `eef/src/model.rs::ModelStatus` | Existing startup launches configured GGUF slots; warm/unloaded/unavailable is not a complete measured lifecycle. Add on-demand ownership, pinning and idle unload. |

Paths above are relative to `crates/`. At the v0.4.0a baseline,
`model_manager.rs` depended on `NodeDashboard`, which also owned shared live state
and submissions. The first development checkpoint below moves that ownership
into runtime services while preserving HTTP routes and approval rules. An HTTP
API alone does not satisfy the target separation.

## Implementation order and acceptance

Command/service separation is a cross-cutting requirement from the first slice,
not a separate rewrite prerequisite. Each row must compile and remain compatible
before proceeding. "Planned" includes extensions to working baseline components.

| Order | Increment | Acceptance boundary | State |
| --- | --- | --- | --- |
| 1 | Identity + advertised address | Local coordinator connection and different advertised address coexist; rename/address change preserves identity; command config/readback; old configs migrate. | Implemented in development; one-PC command tests passed; physical acceptance pending |
| 2 | Coordinator advertisements + network view | Authenticated, bounded, fresh node listings and coordinator hints; authorized peer lookup; no automatic trust or claimed cluster election. | Scoped pull discovery, registration metadata and freshness implemented; owner grant/revoke and restart CLI added in development; no direct peer transport claim |
| 3 | General `models[]` | Multiple multi-capability models per node/backend; normalized old selections; capability-based scheduling; unsupported metadata stays unknown. | Planned |
| 4 | Default lightweight LM | Configurable verified bootstrap; CPU inference; no manual model choice needed in normal setup; offline/disk/cancel failure leaves core usable. | Planned |
| 5 | Structured request interpreter | Versioned bounded schema; original text retained; invalid/time-limited model output rejected; explicit commands work without a model. | Planned |
| 6 | Planning + execution authorization | Existing durable jobs persist plan/grant before dispatch; scope enforced by executor; observable acknowledgements; expiry/replay/ownership tests. | Planned |
| 7 | Local workload runtime | Authorized bounded pipelines execute locally with progress, pause/stop acknowledgement, resource limits and safe restart reconciliation. | Planned |
| 8 | Authorized peer data paths | Pair-specific authentication, scoped transfer limits and expiry; address knowledge alone cannot open a session. | Planned |
| 9 | Continuous workloads + listening | Local camera/audio/sensor loops, event reporting and backpressure; repeat payloads stay local; listening-off leaves services working. | Planned |
| 10 | Full model lifecycle + unload | Measured transitions, load cancellation, active-use pinning and configurable idle unload; scheduler considers startup cost and actual usage. | Planned |

Minimal load/readiness/error reporting is needed in steps 3-7; do not postpone
basic execution correctness until step 10. Step 10 adds complete lifecycle
control and optimization, rather than enabling the first usable inference.

## 1-2. Identity, addresses and discovery

Keep `node_id`, mutable display name, and advertised address separate. Retain
existing `device-*` IDs: the proposed `node-*` example is not grounds to reset
identity, job ownership or resources. Preserve the existing wire `name` field
through compatible mapping if the public contract calls it `display_name`.

An outgoing EEF endpoint, bind/listen endpoint, observed socket source, and
advertised reachable endpoint are distinct facts. Do not turn an outgoing
connection's ephemeral source port into a peer service port. Validate IPv4,
IPv6 and hostnames, with explicit service ports where needed; an address alone
does not imply an active listener. No Radmin/Playit/Hamachi/LAN protocol modes.
Radmin is the owner's preferred PC deployment example, not a runtime dependency
or a claim that the app configures an overlay/tunnel automatically.

Support a node connecting to local EEF via `127.0.0.1:51335` while advertising an
owner-selected overlay address. Never substitute that loopback address for a
remote peer endpoint. Reachability is requester/path-specific, time-bounded and
distinct from authenticated identity. Keep the existing EEF pre-connect probe;
successful probing does not authorize operations.

An optional coordinator advertisement contains coordinator identity, advertised
service endpoint, reported active/standby/unknown state and freshness. It is
discovery metadata, not proof of leadership or authority to change trust roots.
Newly learned coordinators still need the existing owner-approved trust/pairing
path. Do not implement silent failover by trusting a registration string.

EEF's node view includes identity/name, advertised endpoints, online/stale
status, capabilities, models and optional coordinator presence. Update it from
authenticated registration/heartbeats, bound collection sizes, expire stale
records, and define compatible defaults for older nodes. Disclose only fields
needed by the requesting authority; do not distribute PSKs, adapter paths,
tokens or an unrestricted address directory. Address updates alone must not
open connections to arbitrary targets: validate endpoint scope and use bounded,
authorized reachability checks, including DNS resolution/rebinding handling.

## 3-5. Models and interpretation

Extend the current inventory instead of inventing one fixed slot per model type.
Identify a model instance by node, backend and backend model ID; preserve useful
display names. Separate capabilities, input/output modalities, optional roles,
availability, lifecycle and known resource estimates. Examples include language,
vision, detection, OCR, STT, TTS, embedding and classification. One model can
provide several capabilities; advertising metadata does not implement a backend
or grant device access. Map legacy text/VLM selections without widening grants.

The lightweight LM is a configurable model role, not a hardcoded model ID.
Reuse a suitable owner-selected installed model where possible; otherwise use
a versioned bootstrap manifest with source, license, size, digest and runtime
requirements. Select the actual default artifact only after license review and
CPU/startup/memory/interpretation tests. Prefer existing reachable Ollama and
retain local GGUF/llama.cpp; never require CUDA or install a second provider
unnecessarily. The manifest and role binding must be replaceable.

Normal PC setup should automatically bootstrap the default with clear download
size/progress and cancellation, verified bytes and bounded disk usage. Respect
explicit offline/skip/managed-install policy. Upgrading an existing opt-out or
custom setup must not trigger a surprise model replacement. No actual model
has been selected or downloaded by this roadmap update. A missing/corrupt model
means "interpreter unavailable," not failed registration or an uncontrollable
node. Residency is configurable and subject to memory limits, not assumed free.

Preserve the original user input and attach a validated interpretation containing
input type, intent, goal, context hints, constraints, suggested capabilities,
semantic complexity, one-shot/continuous intent, and optional plan/method hints.
Treat all model output as untrusted: bound bytes/collections/time, validate
schema, reject invented identifiers/permissions, keep unknown values unknown,
and require clarification for ambiguous consequential actions. EEF revalidates
against live state; hints never override authenticated origin or owner policy.
Neither natural language nor an LM-generated plan is executable shell code.
Do not present the small classifier/interpreter as a heavy reasoning model.

## 6-7. Plans, grants and deterministic execution

EEF uses the structured goal plus actual capabilities, models, freshness/load,
reachability, priority, ownership and resource limits to build the authoritative
plan. The LM may describe semantic complexity, not assert exact compute cost.
Preserve the current scheduler and extend its constraints and measured inputs.

An execution grant binds the existing job/attempt identity, issuer/session
generation, executor node, exact capabilities/resources/model or pipeline,
priority, enforceable limits, reporting policy and expiry/termination conditions.
Persist issuance before dispatch, correlate acceptance/completion to that grant,
and reject duplicate/stale attempts. Effective permission is the intersection
of EEF's grant and current local node policy. Revoking local permission takes
effect without waiting for a model or a coordinator reply.

Design grants so nodes cannot forge coordinator authority using a network-wide
shared secret: use separately bound credentials/authenticated issuer sessions
and reviewed, scoped authorization. Do not improvise a new crypto construction.
Transport reuse does not mean reusing a shared PSK as a peer/lease trust root.

Pause/stop/cancel/reprioritize need executor acknowledgements and explicit safe
boundaries, not only updates to a database status. Already-completed side effects
cannot be undone. Revocation on a partition is bounded by the granted duration:
no immediate remote-stop claim while disconnected. Use monotonic execution
deadlines; reject unsafe clock/expiry conditions. Check limits within local loops
and terminate bounded child work where supported; mark unsupported guarantees.

After disconnect, permit only the already-authorized work until its bounded
expiry/policy requires stopping. After restart, do not revive old leases or
replay uncertain mutations. Reconcile persisted attempts and explicit owner
decisions using the existing recovery rules. Lost acknowledgement is not proof
that work never ran. Do not migrate active work to another executor before its
old authority is known to be stopped/expired; clustering remains a later gate.

## 8-9. Peer paths, continuous work and listening

EEF gives only authorized endpoints and short-lived pair/job/resource-scoped
session credentials. Both peers authenticate each other and validate direction,
scope, generation and expiry; no authorization follows merely from learning an
IP or sharing an overlay. Encrypt payloads, reject replay, bound frames/queues,
connections, transfer bytes/rate and duration. Keep control traffic responsive
under bulk load. Report transfer status/results to EEF. Failed reachability
reports a clear failure or uses an explicitly allowed bounded relay, not an
unbounded fallback or public unauthenticated listener.

Extend durable jobs with local pipelines: camera -> detector -> optional VLM ->
event; microphone -> VAD -> optional STT -> interpretation/filter -> event.
Do not send every normal frame to EEF. Set rate/resolution, memory/disk/CPU
budgets, event deduplication, backpressure, bounded offline buffering and privacy
retention before claiming continuous support. Add recurring triggers to the
same job/attempt system with explicit overlap, missed-trigger and restart rules.

The listening toggle means ambient input on explicitly selected nodes, **not**
EEF service uptime. Default ambient capture is off and requires the local
microphone permission plus an authorized listening workload. EEF may request it
through normal approved commands but cannot override local denial. Listening off
stops ambient capture/processing and releases resources; typed commands, network,
job control and other authorized work remain available. Status must expose which
node is listening. Heavy LLM/VLM residency is never required to remain reachable.

## 10. Model lifecycle

Track UNLOADED, LOADING, READY, ACTIVE, WARM_IDLE and ERROR from backend/process
observations, not desired configuration. Unknown backend state remains unknown.
Bound concurrent loads, report load failure/cancellation, and pin models while
active workloads use them. Unload EEFN-owned heavy processes after configurable
idle time; do not stop a shared Ollama service or disrupt another user's active
work. Backend limitations must be explicit. Lightweight residency is optional.

Schedule using measured cold-start cost, available memory, current load and
expected workload duration. A warm slower node may win a short request while a
cold faster node may win sustained work; these are test scenarios, not hardcoded
node preferences. Enforce budgets even when estimates are absent or inaccurate.

## Acceptance and remaining scope

Every slice needs unit/protocol tests, old-config/wire migration tests and
command-driven integration without a browser or an available interpreter.
Keep baseline authentication, origin, resource privacy, permission, disk,
updater/installers and no-unsafe-replay tests. Keep existing UI smoke tests but
do not expand visual polish work. Test public commands rather than relying only
on test helpers directly accessing internal modules.

Physical multi-node gate: user command/NL on Node A -> interpretation -> EEF
validation/plan/grant -> Node B load/local execution -> status/result -> Node A.
Record actual node IDs and remote execution evidence, network path, model/runtime
versions, permission decisions and measured payload/event counts. Also verify a
continuous pipeline keeps repeated payloads local while EEF receives bounded
useful events. One-PC fixtures and mocked inference remain separately labelled.

Negative gates: wrong/expired/replayed grants, unauthorized peer lookup/session,
permission revocation, malformed/hallucinated interpretation, LM/backend crash,
disk/memory limits, disconnect/partition, stale coordinator hints, cancelled
loads, duplicate submissions and safe job reconciliation after both-role restart.
The owner will be needed for the physical second-PC/real-media acceptance; do not
claim localhost tests satisfy it.

Wi-Fi/Bluetooth capabilities, ESP32/ESP-NOW/multi-hop, sophisticated distributed
optimization/migration and quorum-based coordinator replication/election/fencing
remain later work. Preserve them in the backlog; advertisements and endpoint
priority do not implement safe active/standby. Defender/signing investigation
remains a release gate, not UI work. Do not change published artifacts, stable
feeds or contributor/account isolation as part of this roadmap revision.

## Development checkpoint: 2026-09-11

- Extracted runtime-owned `NodeService` from dashboard state/config/remote
  management. Model manager and engine use the service; HTTP is an adapter.
  `NodeDashboard` remains a compatibility alias. Not every command family has
  been extracted/exposed yet.
- Added [network commands](COMMANDS.md), offline instance-lock protection,
  running-node commands, JSON output, and `--no-ui` API-only operation.
- Validated independent advertisements and optional coordinator hints pass
  through registration to EEF's existing node view. Old configs need no rewrite;
  old node IDs remain unchanged. Saved/applied addresses differ until restart.
- 91 locked Rust tests passed. Public-command integration passed in
  `.validation/node-commands-NNknMl/`; existing browser mocks and real-process
  first-run regression passed in `.validation/first-run-PSCYYk/`. No physical
  second-PC, peer-session, default-LM or lease claim follows from these tests.
- No release, model download, installer replacement, signing or auth change.

Next: more shared commands, general model metadata and the remaining ordered
increments. Published v0.4.0a stays unchanged.

### Scoped discovery follow-up

- Added `eefn network peers` through existing authenticated submission, using
  immutable origin and exact coordinator-owned disclosure grants. Default is
  self-only, without symmetric/transitive grants or automatic coordinator trust.
- Sanitized live-connection records exclude private bindings and observed source
  endpoints. Monotonic heartbeat age removes stale addresses/model summaries;
  disconnect removes the record. Pages and policy/registration sizes are bounded.
- Grants configure disclosure, never direct execution. Policy/revocation applies
  on coordinator restart. The subsequent command checkpoint below adds owner controls.
- 97 locked Rust tests passed; command integration passed in
  `.validation/peer-discovery-lXlWL2/` and `.validation/node-commands-VEVMDV/`.
  The former runs two logical nodes on one PC, not a physical two-PC test.
- Existing first-run/browser regression passed in `.validation/first-run-IbZSxV/`;
  dashboard initialization/copy-stability mocks also passed. No visual UI work.
- Authentication still inherits the shared-PSK trust boundary; exact-ID grants
  do not isolate mutually untrusted key holders. Independent peer/issuer
  credentials remain required before direct paths or execution leases.

### Owner commands and real-network checkpoint

Unreleased follow-up adds `eef discovery show/grant/revoke`, `eef restart` and
owner-approved `eef node restart`. Configuration edits use the latest saved state
under a shared lock, preserve unrelated settings, and expose saved/applied policy.
Exact-ID disclosure still needs EEF restart to apply. Node restart reports request,
acknowledgement and observed completion separately and does not replay uncertainty.
Outbound queue waits are bounded; cancelled waiters release pending correlations.

109 Rust tests passed. Live testing on the owner's two PCs confirmed registration,
temporary one-way discovery followed by revocation, automatic reconnection across
two EEF restarts, and authenticated remote ping samples. The real node denied
restart without its local management approval; approved restart was validated in
isolated real-process tests, not yet on the second physical PC. No user permission
was enabled remotely. The published coordinator executable was restored after the
live development test. General model metadata remains the next ordered increment;
full workloads, direct peer transfers and independent credentials are still pending.

## Installation direction: MSI

The owner requested MSI on 2026-09-11. Add separate EEF and EEFN MSI packages in
a packaging increment, retaining current installers until replacements pass
acceptance. Reuse verified payloads and inventories rather than a second build
of the application. MSI tooling has not been installed; no MSI was built here.

Acceptance: per-user dedicated-directory setup where supported, explicit startup
choice, stable product/upgrade identity, repair/uninstall, preserved config/models/
job data, locked-file/restart handling, old EXE-install migration, unattended
installation and truthful errors. Do not automatically delete user data on
uninstall. Test both roles together and upgrades on a second PC. Signing, hashes
and Defender checks still apply; MSI is not an antivirus or UAC bypass. Core
command work remains the immediate priority, not installer UI polish.
