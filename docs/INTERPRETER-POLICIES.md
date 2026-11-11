# Interpreter service policies

This is an unreleased development increment for command-first network
interpretation, accepted at source `50174aa` in the
[four-job hosted run](https://github.com/eepyfishy/eef/actions/runs/42552611503)
and independent source/log audit. It records bounded coordinator owner consent.
The running-node [explicit commands](NETWORK-INTERPRETER-COMMANDS.md) and
opt-in [normal input workflow](NETWORK-INPUT-WORKFLOW.md) now consume that consent
in accepted development source. Published alpha installers do not include these
later checkpoints; physical, real-model and stable-release gates remain open.

## Commands

Use the running coordinator's configuration, as with other owner commands:

```powershell
eef --config <coordinator-config> interpreter-policies show
eef --config <coordinator-config> interpreter-policies list
eef --config <coordinator-config> interpreter-policies usage
eef --config <coordinator-config> interpreter-policies approve --origin <input-node-id> --provider <serving-node-id> --max-calls 20
eef --config <coordinator-config> interpreter-policies show <policy-id>
eef --config <coordinator-config> interpreter-policies revoke <policy-id>
```

Repeat `--origin` and `--provider` for multiple explicit stable node IDs. Both
lists are required; there is no implicit all-connected-nodes scope. Listing
supports `--limit` (1–100) and `--after-generation`. Add `--json` for the typed
report; ordinary output explains retained approval and revocation state.

New approval requires the coordinator's private credentials to be configured
and applied. The complete saved and applied authentication policies must match,
including credential additions, rotation, revocation and legacy settings.
Saved authentication changes must be applied before another approval.
Inspection and revocation remain available without a current model or issuer.
Saving a policy does not contact the listed nodes or verify current availability.

## Limits and receipts

Every approval has finite per-call, concurrency and aggregate limits. Defaults
are 48 KiB task bytes, 1,024 output tokens, 30 seconds execution, 64 KiB result
bytes, two concurrent calls globally, one per input node and one per serving
node, and 100 calls in total. Aggregate token, execution-time and result-byte
allowances default to the call count multiplied by each per-call limit. The
command exposes overrides for all limits; `--help` lists them. These values do
not widen the enclosing execution grant's existing bounds.

`--valid-for-seconds` adds an expiry. Omitting it leaves the policy without a
time expiry but retains finite aggregate limits. The
[allowance ledger](INTERPRETER-ALLOWANCES.md) reserves those budgets before
broker delivery and supports read-only usage reporting. Saving an approval
itself starts no service call; inference requires a separate source-bound
running-node request and the serving node's current permission.

`interpreter-policies usage [policy-id]` inspects conservative reserved budgets
and bounded attempt summaries without prompts or complete grants. Omit the ID
for the latest policy; historical and revoked policy usage remains inspectable.
Use `--limit` and `--after-sequence` to page through retained attempts. Unknown
completion is not proof that shared inference stopped and retains its slot.

The CLI reads the current generation/runtime and sends one mutation to that
same runtime. Concurrent approval with an outdated generation fails. A new
generation supersedes earlier generations without deleting them. Supplying an
explicit `--operation-id` lets an owner inspect the same receipt after a lost
reply. Reuse with the same approval intent returns retained history, including
later revocation; changing scope or limits under that receipt is refused.
Receipt reuse never renews expiry, resets allowances or starts inference.

Approval, supersession and revocation use the existing durable job journal and
its shared count/byte limits. Each approval reserves bounded revocation space.
Revocation and read-only receipt reuse still work after limits are lowered.
Damaged records are retained and refused rather than replaced with defaults.

## Use in network interpretation

An additional unreleased [routing command](INTERPRETER-ROUTING.md), accepted at
`0aeb23a`, can inspect a fresh policy/disclosure-scoped advertised candidate.
It records no allowance or attempt and starts no inference.

This policy grants no filesystem, process, microphone, camera or other tool
permission. A serving node must separately enable interpreter sharing and retain
its local model and grant controls. Advertisements alone grant nothing.

The accepted broker binds independently authenticated provider/model selection,
durable reservations and each stage's immutable source and original context
before granting work. Uncertain attempts are not automatically replayed.
An exact completed semantic result may be explicitly applied through
[retained-result recovery](INTERPRETER-RESULT-RECOVERY.md); that does not renew
the old continuation or approve tool execution. See
[current verification boundaries](STATUS.md) for exact accepted checkpoints and
the remaining real-model, physical, onboarding and stable-release requirements.
