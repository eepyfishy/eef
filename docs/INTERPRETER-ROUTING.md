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
