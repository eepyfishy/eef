# Interpreter service policies

This is an unreleased development increment for command-first network
interpretation, accepted at source `50174aa` in the
[four-job hosted run](https://github.com/eepyfishy/eef/actions/runs/42552611503)
and independent source/log audit. It records bounded coordinator owner consent;
automatic network interpretation is still unavailable.

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
time expiry but retains finite aggregate limits. The current increment records
these limits. The new [internal allowance ledger](INTERPRETER-ALLOWANCES.md)
implements reservations and read-only usage reporting with verification pending;
production broker delivery is still absent, so saving approval starts no service
call.

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

## Remaining network interpretation work

An additional unreleased [routing command](INTERPRETER-ROUTING.md), accepted at
`0aeb23a`, can inspect a fresh policy/disclosure-scoped advertised candidate.
It records no allowance or attempt and starts no inference.

This policy grants no filesystem, process, microphone, camera or other tool
permission. A serving node must separately enable interpreter sharing and retain
its local model and grant controls. Advertisements alone grant nothing.

The remaining broker must reserve durable allowances before granting work,
resolve fresh independently authenticated nodes and exact model selections,
bind every stage to its immutable source and original context, and settle
uncertain attempts without automatic replay. Those paths and production
interpreter lease-exchange coverage remain open. See
[network interpretation](NETWORK-INTERPRETER.md).
