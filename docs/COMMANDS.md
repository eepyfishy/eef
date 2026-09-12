# Commands in v0.4.0a2

This test release supports these commands without a browser or LM. Run
from the installation directory, or supply `--config` to select a node config.
Older v0.4.0a installers do not contain them.

## Unreleased: backend-only operation and connection controls

Both `eef --no-ui` and `eefn --no-ui` keep their command/API services running
without mounting browser HTML, JavaScript, CSS or legacy pages. Node API-only
mode also omits the interactive file-picker endpoint. These are runtime flags;
developers can additionally compile either role with `--no-default-features`
to omit bundled dashboard assets entirely. Default builds retain the dashboard.

```powershell
.\eefn.exe connection show --json
.\eefn.exe connection pause --json
.\eefn.exe connection resume --json
.\eefn.exe connection pair-local --json
```

Connection controls require a running development node and call the same backend
operations as the existing UI. Pause saves disabled outgoing EEF connections;
resume uses the existing endpoints and trust. Both request a node runtime restart
to apply the change, including other pending settings. This does not restart
Windows or shut down the local command API. Inspect the returned saved policy
separately from live connection state; acknowledgement is not reconnection.

`pair-local` explicitly clears selected EEF endpoints and enables automatic
same-user local pairing. It is an owner configuration change, not a network scan
or trust grant for arbitrary peers. It must not be used merely to inspect status.
No connection command implicitly downloads models or executes inference.

Saved UI preferences are `dashboard.ui_enabled` on the node and `web.ui_enabled`
on EEF, both defaulting to true. False hides the browser while keeping commands
available. Apply saved preferences with the role's restart command. On the node,
the browser route gate changes without rebinding its command listener; that node
runtime restart still applies other pending settings. `--no-ui` overrides a saved
true value. Builds without dashboard assets cannot enable the browser.

Compatibility: existing node `dashboard.host/port/enabled` fields still configure
the local HTTP listener. Legacy `enabled:false` disables that listener, including
commands; use `ui_enabled:false` or `--no-ui` to hide only the browser. No old
explicitly disabled listener is silently enabled by the new preference.

## Unreleased: jobs through the running node

```powershell
.\eefn.exe jobs list --json
.\eefn.exe jobs find --operation-id RECEIPT_UUID --json
.\eefn.exe jobs generate-text --prompt "Explain the current task" --role request_interpreter --json
.\eefn.exe jobs get JOB_ID --json
.\eefn.exe jobs output JOB_ID --json
.\eefn.exe jobs pause JOB_ID --json
.\eefn.exe jobs resume JOB_ID --json
.\eefn.exe jobs stop JOB_ID --json
.\eefn.exe jobs remove JOB_ID --json
```

These deterministic commands need the development node; `generate-text` and
`output` also need the development coordinator. They use the running node's
actual local command endpoint and existing authenticated EEF connection, never
launching a second node or guessing an executor from its hostname. They work in
API-only mode. Node-local management approval is not required to control this
node's own jobs; execution still requires the executor's normal permissions.

EEF stamps the origin from the connection. List, status, controls and output are
restricted to jobs submitted by that stable origin node, not jobs merely executed
there or created by the coordinator owner. `generate-text` optionally accepts
`--target-node`, `--backend` and `--model`; `--role` is an exact advertised model
label, not authority. This creates an existing durable text-inference job, not an
automatic natural-language plan. Prompt size is bounded to 32768 bytes.

Acknowledgement is not completion. Inspect `data.status`; pausing/stopping may
remain in progress until the current step finishes. Stop does not undo remote
effects. Resume retains completed steps and follows the existing uncertain-work
rules. Remove deletes only finished job history and cannot stop an active job.
List returns the existing bounded view (first 100 visible jobs plus total).

`output` explicitly returns text-model results only: at most 32 tasks and 32768
aggregate UTF-8 content bytes with truncation flags. Pending text is null. Normal
status still omits output; file contents, media and raw parameters are not exposed
by this command. Output can contain sensitive user-requested text; don't publish
it as diagnostics without reviewing it.

Failures distinguish `not_sent`, coordinator `job_rejected`, and unconfirmed
transport outcomes. Mutating errors remain conservatively uncertain unless known
not sent: an error does not prove a checkpoint was rolled back. Inspect list/status
before an explicit retry. Job commands are not automatically resubmitted after timeout or
reconnection; the command operation ID is not a durable deduplication key.

Creation recovery in the current development build: the CLI generates a receipt
`operation_id` before sending, including it in an unconfirmed local-reply report.
With an updated coordinator, creation returns `creation_correlated:true` and the
same ID is persisted as the job's `request_context.request_id`. Use `jobs find
--operation-id RECEIPT_UUID` to search this origin node's retained creation
history without executing anything. Matching happens before the 100-result limit;
`total` and `truncated` describe the matched set. Inspect each returned job status.

Lookup is not an execution receipt ledger: pause/resume/stop/remove commands do
not become new job creations. No match can mean in-flight work, removed history,
a different coordinator, or an older coordinator without correlation support;
it does **not** prove that creation was never accepted. Reusing a receipt does
not deduplicate work and may match multiple jobs. Do not automatically retry an
uncertain mutation. Transport IDs remain independently generated, so reused
correlation cannot mix up replies. Both roles must be updated for lookup; older
coordinators reject `jobs.find` rather than returning an unfiltered list.

## Unreleased: model route inspection

`eef models route --capability llm.infer --role request_interpreter --json`
previews matching connected model instances without inference. Optional `--node`,
`--backend`, `--model` and `--limit` (1-32, default 8) narrow the view. Supported
capabilities are `llm.infer` and `vlm.analyze`. A role must match an explicit owner
label; unknown roles do not match. This grants no execution or reservation and
does not prove model readiness. State may change before actual dispatch.

Durable jobs may use `generate_text` or `analyze_image` with
`constraints.model_role` and the existing node/model constraints. Routing does
not escape the requested role/backend when falling back across tiers. Image
payloads must be supplied by the caller; this does not capture camera images or
implement automatic interpretation/planning. Node permissions still apply.

## Unreleased: coordinator discovery controls and restart

These additions are in the development tree, **not the published v0.4.0a2
installers**. They work with existing v0.4.0a2 nodes; only EEF needs the new code.

```powershell
.\eef.exe discovery show --json
.\eef.exe discovery grant --requester NODE_A --target NODE_B --json
.\eef.exe discovery revoke --requester NODE_A --target NODE_B --json
.\eef.exe restart --json
.\eef.exe node restart --node NODE_B --json
.\eef.exe node restart --node NODE_B --wait-seconds 60 --json
```

Use exact stable IDs. A grant lets A inspect B, not B inspect A, and never grants
execution or direct peer access. Self visibility is implicit and cannot be
revoked. Duplicate grants and absent revocations are no-ops. Bounds and secret
redaction are unchanged. No node permissions or connection settings are modified.

The running coordinator owns these commands. `show` distinguishes `saved_policy`
and `applied_policy`; `restart_required` refers to discovery policy only. Saving
a grant or revocation does not activate it. `restart` acknowledges the request,
not completion: verify a new diagnostics `runtime_id` and node reconnection.
Existing jobs are not automatically replayed. Plan restarts around active work.

`node restart` restarts the **node app, not Windows**. The node owner must first
enable Settings > Management from EEF > "Allow EEF to apply settings and restart
this node." This existing permission also permits remote settings changes; the
command cannot enable it itself. Denial returns `approval_required` with exit 1
and sends no restart. Restarting can interrupt active work; use an idle node.

The default wait is 30 seconds (0-60 supported). Results carry an operation ID,
stable node ID, prior/new runtime IDs, `restart_requested`, `acknowledged`,
`completed` and `outcome_unknown`. With a nonzero wait, success means a connected
node with the same stable ID and a new runtime ID was observed. With zero wait,
success means acknowledgement only, with `completed:false`. A timeout or lost
reply does not prove failure to restart: inspect diagnostics before any retry.
No restart is automatically resent, and these operation IDs are not durable
deduplication keys. Completion does not verify recovered jobs or model readiness.
POST `/api/commands/node/restart` accepts `{node_id,wait_seconds}` under the
existing local owner/Origin/Host guard. The command works with v0.4.0a2 nodes'
diagnostics and management protocol; no new remote listener is introduced.

GET `/api/commands/discovery` returns this policy view and `runtime_id`. POST
accepts `{schema_version:1,expected_runtime_id,command:{operation,requester,target}}`;
the CLI reads the runtime ID automatically. `show` has no requester/target fields.
Wrong schema/runtime IDs, unknown fields and untrusted origins are rejected.
No automatic retry after a failed mutation. Read state before retrying explicitly.

The core service serializes edits and reads the latest saved configuration before
changing one grant, preserving unrelated pending settings. This protects concurrent
commands within the running coordinator; it is not distributed configuration
locking or a merge guarantee for external file editors/full-config replacements.
Corrupt or missing saved config is rejected rather than recreated from defaults.

`tools/test-live-discovery.mjs` is an explicit live-network acceptance test. Set
`EEF_LIVE_TEST=1`, `EEF_LIVE_EEF_BINARY`, `EEF_LIVE_EEF_CONFIG`,
`EEF_LIVE_NODE_BINARY`, `EEF_LIVE_NODE_CONFIG` and `EEF_LIVE_REMOTE_NODE` to
owner-selected local paths and the other node's stable ID. Use an idle test
coordinator with no pending changes. It temporarily grants one-way visibility,
restarts EEF, tests discovery/pings, revokes the grant and restarts again. It
attempts grant cleanup on failure and records local evidence under `.validation`.
It does not deploy binaries, update remote nodes, run media/model workloads, upload data
or infer physical device count from IDs alone. Do not run against production jobs.
Set `EEF_LIVE_RESTART_NODE=1` additionally to request a remote node restart after
grant cleanup. With local management approval it verifies the new runtime ID;
without approval it verifies denial and sends no restart. This is opt-in because
it can interrupt work on the other node.
Set `EEF_LIVE_MODEL_INVENTORY=1` to also verify read-only model registration
inventory on both nodes. This requires the development coordinator command below;
it does not scan backends, download models or run inference.

## Unreleased: local node restart and actual API discovery

```powershell
.\eefn.exe restart --json
.\eefn.exe restart --wait-seconds 60 --json
```

This is a local owner command: remote-management approval is not required. It
restarts the node app, not Windows, and applies all pending node settings. It
refuses to start a stopped node. It sends one restart request, never an automatic
retry. The existing browser and approved remote restart paths call the same core
restart operation; their permission guards remain unchanged.

The default completion wait is 30 seconds, with 0-60 supported. Zero waits for
acknowledgement only. A positive wait requires the same stable node ID and a new
initialized runtime ID. Results distinguish requested, acknowledged, completed
and unknown outcome. Timeout/lost replies return failure with an uncertain outcome,
not proof that restart did not happen. Completion does not verify EEF reconnection,
workloads or model readiness. If the restart disables the local API, local
completion cannot be observed; inspect state before any explicit retry.

POST `/api/commands/restart` takes
`{schema_version:1,expected_node_id,expected_runtime_id}` and checks the current
node/runtime before queueing. Null runtime is allowed only when the current node
has not initialized one, allowing a repaired startup configuration to be retried.
Operation IDs in CLI output are correlation IDs, not durable deduplication keys.

New nodes record their bound local API beside the configuration as
`CONFIG_FILENAME.api.json`. Commands consult it only while the instance lock is
held by the running node, validate identity and loopback scope, and reject corrupt
or oversized records. Thus saved port/disable settings do not redirect commands
away from the still-running API. Restart completion polling follows the updated
record if the port changes. This is local discovery, not a new remote listener.
Missing records fall back to config for older nodes; stopped-node commands ignore
stale records. Failure to write discovery is logged without stopping the node.

## Unreleased: node model selection commands

The development coordinator also exposes these operations for a connected node:

```powershell
.\eef.exe node models --node NODE_ID show --json
.\eef.exe node models --node NODE_ID select-ollama --model OWNER_MODEL_ID --modality text --role request_interpreter --json
.\eef.exe node models --node NODE_ID hints --backend ollama --model OWNER_MODEL_ID --clear-roles --json
.\eef.exe node models --node NODE_ID provider auto --json
.\eef.exe node models --node NODE_ID remove --backend ollama --model OWNER_MODEL_ID --json
```

Both EEF and the target node need the development version. `show` is read-only
over the existing authenticated coordinator connection. Changes require the
node owner's existing management approval. EEF first inspects support/approval;
the node checks current approval again under its configuration lock before
applying the change. Approval cannot be granted by these commands. Older nodes
fail with `model_commands_unavailable`, without a legacy full-config save fallback.

Local and remote model commands share their argument adapter and node core
operation. Remote `select-gguf --file`/`--projector` paths refer to existing files
on the **target node**, not on the coordinator. Removal never deletes model files.
No download, model load, provider switch or restart is implicit. Run the separate
approved `eef node restart --node NODE_ID` command when ready to apply all pending
settings. Saved selections and registered models remain distinct.

The EEF result wraps the node report in `data` and includes `mutation_requested`,
`acknowledged` and `outcome_unknown`. A mutation is sent once; a lost/ambiguous
response returns `model_change_unconfirmed`, not an automatic retry. Inspect
saved selections before retrying. This does not promise durable deduplication or
exactly-once writes. Approval denial is known not to apply the selection change.

These commands operate on the existing Ollama/GGUF configuration through the node
core service, without JSON editing or a browser:

```powershell
.\eefn.exe models show --json
.\eefn.exe models select-ollama --model OWNER_MODEL_ID --modality text --role request_interpreter --json
.\eefn.exe models select-gguf --model OWNER_MODEL_ID --file C:\Models\owner-model.gguf --json
.\eefn.exe models hints --backend ollama --model OWNER_MODEL_ID --capability llm.infer --role request_interpreter --json
.\eefn.exe models hints --backend ollama --model OWNER_MODEL_ID --clear-roles --json
.\eefn.exe models provider ollama --json
.\eefn.exe models remove --backend ollama --model OWNER_MODEL_ID --json
```

Selection commands do not download/load models or restart the node. They report
`saved_selections` separately from `registered_models`; registration is not proof
of loaded state. Apply pending changes with `eefn restart`. Offline commands
hold the instance lock and save for its next start. An existing saved identity is
required (normally created by setup; `network set` can initialize it).

Provider selection remains `auto`, `ollama`, or `llamacpp`; auto prefers an
available Ollama service. Selecting a model does not silently change provider.
Ollama selection names need not be installed yet; unavailable selections are not
advertised on connection. GGUF requires an existing file and can accept
`--projector`, `--gpu-layers`, `--context`, and an optional advanced `--port`.
Without a port, the command reuses the model's saved internal port or briefly
allocates a loopback port; availability is checked again when the backend starts.
It never launches a model server during selection.

Repeated `--capability` and `--role` flags form bounded lists. Capabilities may
restrict existing adapter support, not add unsupported executors. `--clear-capabilities`
on `hints` disables that selection's inference capabilities; it does not remove
the model or unload it immediately. Role labels do not grant permissions or select
a default interpreter yet. Empty roles explicitly clear labels. Re-selecting an
existing model without selection hints preserves its current hints.

The local owner API is POST `/api/commands/models`, taking
`{schema_version:1,expected_node_id,command}`. Operations are `show`,
`select_ollama`, `select_gguf`, `hints`, `remove`, and `provider`; see the shared
typed commands in `eefn/src/model_selection.rs`. Edits use the same config lock as
other node settings, preserve unrelated values, reject corrupt/missing files,
and avoid writing on no-ops. Removing a selection never deletes model files.
Output omits GGUF/projector paths, backend service URLs and credentials. A failed
or timed-out response is not permission to automatically retry a mutation.

Published older nodes do not enforce these new selection restrictions. Do not
downgrade a restricted node and assume that its restrictions remain enforced.

## Unreleased: registered model inventory

```powershell
.\eef.exe models list --json
.\eef.exe models list --node NODE_B --json
.\eef.exe models list --capability vlm.analyze --limit 8 --json
.\eef.exe models list --after NODE_A --json
```

GET `/api/commands/models` is the same read-only owner operation; optional query
fields are `node_id`, `capability`, `after` and `limit`. The node filter and cursor
are mutually exclusive. Pages contain 1-32 nodes (default 8), each with its models,
within 512 KiB. Follow `next_after`; pages are not a frozen network snapshot.
Disconnected/unregistered target nodes fail rather than appearing as empty models.
A connected, registered node with no selected/advertised models has `models:[]`.

Each model instance uses the tuple `(node_id, backend, model_id)`, so the same
model name on two backends/nodes remains distinct. Duplicate identities in a
registration are rejected, not silently merged. Registration freshness is shown
separately; stale metadata is not readiness or proof that inference will work.

Inventory uses [versioned model metadata](MODEL-METADATA.md) when supplied, or
normalizes legacy text/VLM hints when it is absent. `metadata_source` identifies
which path was used. Unrecognized legacy modality supplies no capability hints.
It does not inspect every installed file or backend, invent roles, measure
residency/memory, or verify advertised capabilities. Unreported
roles, lifecycle and resource estimates are `null`, not zero. Private paths,
network addresses, configuration and credentials are not projected.

The command loads/downloads no model and grants no permission. The subsequent
metadata increment makes existing text/vision routing respect advertised
capabilities/state; general model configuration, role bindings and new capability
executors remain later work. Published v0.4.0a2 installers do
not include this new coordinator command; their nodes remain compatible.

## Node network commands (v0.4.0a2)

```powershell
.\eefn.exe network show
.\eefn.exe network set --name "Laptop" --advertise-address 26.1.2.3
.\eefn.exe network set --coordinator-id eef-laptop --coordinator-address 26.1.2.3:51335 --coordinator-state standby
.\eefn.exe network show --json
.\eefn.exe network set --clear-address --clear-coordinator
.\eefn.exe --no-ui
```

## Diagnostics and private pairing

```powershell
.\eefn.exe network diagnose --json
.\eef.exe diagnostics --json
.\eef.exe diagnostics --node YOUR_NODE_ID --samples 5 --json
.\eef.exe invite --address YOUR_COORDINATOR_ADDRESS:51335 --json
```

EEF commands contact its running loopback API; node diagnostics also work while
stopped, with `running:false` and no live measurements. `--config` selects an
explicit installation. Reports are printed locally, never automatically uploaded.
They include stable node/runtime IDs, software version, basic state and numeric
counters, excluding machine names/addresses/paths/secrets/raw errors. Counters
last for the node-service process: `connection_checks` counts checking events,
`successful_connections` counts connections, and `failed_connection_attempts`
counts disconnected-error events, not unique networks or exhaustive packet loss.

Explicit ping probes sample 1-10 `system.ping` calls with a two-second timeout
per sample and report round-trip milliseconds, versions and failures. They do
not test inference or perform device I/O. A failed sample makes CLI exit nonzero.
GET `/api/diagnostics` exists on both local APIs; EEF additionally accepts POST
`/api/diagnostics/probe` with `{node_id,samples}`. Reports contain pseudonymous IDs;
review before sharing. Raw configs/logs are not support reports.

The invite command accepts an explicit reachable host:port (IPv6 in brackets).
POST `/api/network/invite` also accepts `{address}`; omission keeps the hostname
fallback for existing clients. A pairing code contains the network secret and
is deliberately NOT a diagnostic report. Share it privately only with the node
owner; never publish it in logs, issues or release notes. Address selection does
not configure firewall/overlay software or test reachability.

Addresses are examples, not defaults. IPv6 endpoints use `[address]:port`.
Coordinator advertisements require a port; node advertisements may be a host
alone. Nothing opens a peer listener or configures an overlay. Active/standby
state is owner-reported metadata, not verified leadership or new authority.

`set` edits only supplied fields, preserving ID, connection endpoints, secret,
models, permissions and other config. `show` omits PSKs/private resource bindings.
Stopped-node commands hold the instance lock; an initial `set` creates ordinary
persistent identity/config without starting models, discovery or listeners.
`show` does not create identity. Offline edits apply at next start.

Running-node commands use its guarded loopback API, reading the configured port
automatically. They never register a second node or edit around its lock. Online
edits report `restart_required` and distinguish saved `network` from
`applied_network`. Restart applies advertisements without changing identity.
If the API is explicitly disabled or an old release lacks the endpoint, commands
fail rather than enabling it or bypassing the running process. Stop to edit offline.

`--no-ui` omits HTML/JS/CSS/legacy pages and ignores `--open-dashboard`, while
keeping local API guards. Existing `dashboard.enabled=false` still disables
the entire API. Other lifecycle command families are not implemented yet;
their existing APIs remain available without a browser.

`--json` prints one result on stdout; logs go to stderr. Success exits 0. Command
failures exit 1 with `schema_version:1`, `success:false`,
`error_code:"command_failed"` and an error description. Clap syntax errors retain
normal behavior (exit 2). Expected node ID is a wrong-instance check, not authentication.

## API and registration

`POST /api/commands/network` retains the existing loopback/Origin/Host guards:

```json
{
  "schema_version": 1,
  "expected_node_id": "your-persisted-node-id",
  "command": {
    "operation": "set",
    "changes": {"advertised_address": "26.1.2.3"}
  }
}
```

Inspect with `"command":{"operation":"show"}`. Unknown fields, conflicting
set/clear operations, malformed addresses and wrong targets are rejected. HTTP
errors retain the existing API shape/status, not the CLI wrapper.

Registration has additive `network:{advertised_address,coordinator}` metadata;
coordinator fields are `coordinator_id`, `address`, `state`. Missing metadata means
no advertisement; missing coordinator state means unknown. Gateway validation
keeps it separate from observed source IP/port. EEF world/status entries expose
it; disconnected entries stay marked offline. Node status reports applied values.
Existing owner-approved remote configuration may also propose `network` changes.

Metadata does not initiate connections, establish trust, authorize execution or
implement failover. Scoped discovery is now available as described below.

## Inspect the registered network

```powershell
.\eefn.exe network peers --json
.\eefn.exe network peers --node node-b --json
.\eefn.exe network peers --limit 1 --json
.\eefn.exe network peers --after node-a --json
```

Requires a running node connected to EEF; offline commands do not start a new
connection for discovery. Requests use the existing authenticated submission
channel. EEF uses server-stamped origin, not a caller-supplied requester ID.
The local API accepts `command:{operation:"peers",query:{limit:32}}`; optional
query fields are `node_id` and `after` (mutually exclusive).

By default a node can inspect **only itself**. EEF's owner can configure exact,
directional disclosure grants in EEF configuration, using the existing owner
config API or a config file. The development grant CLI is described above;
published v0.4.0a2 does not have it. Example:

```yaml
discovery:
  grants:
    node-a: [node-b]
  freshness_seconds: 30
```

This allows A to inspect B, not B to inspect A or A to inspect B's other peers.
No wildcards or implicit transitive grants. Use actual persistent node IDs.
Policy changes, including revocation, apply after **EEF restart**, not when
saved. Invalid policies fail validation; no fallback to unrestricted discovery.
Limits: 256 requesters, 256 targets each, 4096 total grants, 256 KiB policy and
freshness of 1-300 seconds. Policy is local to this coordinator, not replicated.

Results contain a bounded, sanitized registration projection: ID, display name,
advertisement/coordinator hints, capabilities and model ID/backend/modality.
They omit observed connection source IP/port, PSKs, private adapter bindings and
arbitrary model fields. Registration limits are 128 capabilities (128 bytes
each), 64 models (ID 256 bytes, backend/modality 64 bytes each), and 256-byte
display names. Oversized/malformed registrations are rejected, not silently
advertised as complete.

Only currently registered connections are considered. Monotonic heartbeat age
drives `online`/`stale` status; `online` means recent control-channel activity,
not verified reachability from another node. Stale entries retain ID/name but
lose addresses, coordinator hints, models and capabilities. Disconnected entries
disappear rather than returning historical addresses. Use `remaining_fresh_ms`
conservatively and re-query; discovery is not an authorization cache.

Pages have at most 32 entries and a 512 KiB response budget. Continue with
`next_after` until null; cursors are lexicographic IDs, not a snapshot or grant.
Each page rechecks applied policy/current connections. Concurrent registrations
may change results; start a new listing when a complete current view is needed.
Missing and denied individual lookups use the same error to avoid leaking
whether an unauthorized ID exists. `direct_access_authorized` is always false:
knowing a peer address does not authorize a connection, transfer or workload.

Trust boundary: this increment inherits v0.4.0a's shared-network-PSK identity
authentication. It does not provide independent cryptographic identities for
mutually untrusted key holders; a holder of that shared key can authenticate as
an otherwise unused node ID. Exact-ID disclosure policy is not a substitute for
that future security work. Do not use these grants as tenant isolation or expose
an unauthenticated peer listener. Separate peer/issuer credentials remain a
prerequisite for direct data paths and execution leases.

`node tools/test-peer-discovery.mjs` verifies two logical nodes on one PC with
no UI/models: self-only defaults, directional grants, pagination, forged query
rejection, stale redaction, restart-applied revocation and disconnect removal.
It does not establish physical multi-PC/overlay reachability or direct peer execution.

`node tools/test-node-commands.mjs` validates real local processes/public commands
with isolated config, no model and no browser: offline/headless operation, stable
IDs, address separation, coordinator metadata, saved/applied state, permission
preservation and invalid address/target/Origin rejection. It is not a physical
second-PC or peer-connectivity test.
