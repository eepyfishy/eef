# Interpreter routing observations

This unreleased development increment is accepted at source `0aeb23a` in the
[four-job hosted run](https://github.com/eepyfishy/eef/actions/runs/42581232439)
and independent source/log audit. It adds read-only provider resolution through existing model
ranking, owner service policy and directional disclosure. It does not implement
automatic network interpretation or reserve execution authority.

```powershell
eef --config <coordinator-config> interpreter-policies resolve --origin <input-node-id>
eef --config <coordinator-config> interpreter-policies resolve --origin <input-node-id> --local-only
eef --config <coordinator-config> interpreter-policies resolve --origin <input-node-id> --provider <serving-node-id> --backend ollama --model-id <selected-model>
```

The input node needs no installed model. The owner must first save a current
[interpreter service policy](INTERPRETER-POLICIES.md). Its explicit input and
serving node scopes apply alongside the existing directional discovery policy;
approving service use does not disclose an otherwise hidden node. Saved and
applied authentication settings must match. Revoked, expired or superseded
policies cannot produce a route observation.

Model/backend/provider preferences are exact restrictions. `--local-only` restricts
selection to the input node. A conflicting provider is rejected, and unavailable
explicit selections do not silently choose another model or backend. Without an
explicit selection, the existing model registry ranks compatible candidates by
its current load, latency and warm-state information.

Both input and serving nodes need fresh independently authenticated private
connections, a validated runtime registration, and available grant/lease support.
The selected serving node must additionally advertise opt-in interpreter sharing
and a unique current text-capable `request_interpreter` model selection. A model
invented only in the registry, a legacy connection, a stale registration, a
missing role, an unavailable model or an undisclosed provider cannot qualify.
The command rechecks the selected connection, model and policy before returning;
if the selected route changes, it refuses rather than substituting a provider.

Nodes with sharing disabled omit the new optional serving advertisement. Older
nodes can still connect; missing service metadata does not opt them into serving.
The advertisement describes applied settings. Later saved permission/model edits
may make it stale before reconnect, and every actual grant must still pass the
node's current permission/model checks. Neither an advertisement nor a successful
observation is execution authority, a model reservation or proof of inference.

Plain output identifies an advertised candidate or explains that none qualifies.
`--json` includes policy and connection correlations plus advertised grant limits.
These IDs grant no access. The local-owner API is
`POST /api/commands/interpreter-route` with a strict bounded request and current
coordinator runtime. No dashboard must remain open. Resolution uses existing
state and starts no model, download, scan, background poll or inference request.

The durable allowance/attempt ledger and the grant-backed three-stage broker
remain required before normal model-free input can delegate automatically.
Physical two-PC and real-model acceptance remain separate from synthetic tests.

## Stage compatibility (development)

New registrations can separately advertise `interpreter_task_support`, with
`schema_version:1` and a bounded, nonempty list of distinct positive `versions`.
The current node advertises versions one and two only alongside opt-in private
interpreter service registration. The strict published `interpreter_service`
shape is unchanged; older coordinators can ignore the optional sibling field.
Missing task-support metadata means the published version-one service contract,
never inferred version-two support. Malformed support or support without serving
permission is refused. Unknown positive task versions can be observed but are
not selected for a stage whose required version they do not include.

```powershell
eef interpreter-policies resolve --origin INPUT_NODE_ID --stage method
```

`--stage initial`, `--stage clarification` and `--stage method` apply the current
definition requirement before choosing a ranked interpreter. Omission preserves
the original general observation and its command shape; it does not establish
support for every stage. The ordinary broker always supplies its actual stage.
A pinned older provider can remain eligible for initial interpretation while
being excluded for method preparation. Other policy, disclosure, exact preference,
model, health and capacity checks still apply.

The selected registration is checked again before returning a route and at the
existing allowance/continuation fences before charging or sending work. This does
not authorize a request, reserve capacity, attest a peer's implementation or
downgrade a method to an older definition. Exact task/spec hashes and node-local
permission/grant checks remain mandatory. Unavailable compatible providers leave
input saved; they do not trigger a model download or an unapproved substitute.

Versions identify frozen reconstruction definitions. Future prompt, catalogue or
schema changes need explicit version/compatibility handling; a version claim
alone cannot replace the inference-spec digest check. Physical-network, real-model
quality and stable-release acceptance remain separate gates.

The initial local checks pass 55 focused tests across advertisements, authenticated
snapshots, task definitions, route confirmation and the coordinator's forty
exchange/application/recovery cases. Three public-command fixtures also pass with
simulated models on one PC:

- `network-input-commands-Sgui0c` uses current coordinator and node builds.
- `network-input-commands-o2o8DC` adds the published alpha.7 node as a third logical
  node. Its initial/clarification routes remain available, its method route is
  excluded with zero inference/reservation/issuance, and ordinary input completes
  preparation through the compatible provider before separate owner approval,
  system-information execution and retained-result retrieval.
- `network-input-commands-9jsu7Z` uses the published alpha.7 coordinator with current
  nodes. Registration remains compatible and an explicitly issued version-one
  method task retains its absent schema and frozen prompt. This is old-definition
  execution under the old coordinator's grant, rather than a new coordinator
  retrying a version-two task as version one.

The old executables match the recorded published payload hashes. No installer,
model download or physical node is involved. To reproduce either upgrade
direction with existing binaries, `networkInputCommandWorkflow` in
`tools/network-input-command-workflow.mjs` accepts `legacyNodeBinary` or
`legacyCoordinatorBinary` separately. Its default invocation tests the current
builds and remains part of the hosted request-review fixture. Full hosted
acceptance of this increment remains pending.
