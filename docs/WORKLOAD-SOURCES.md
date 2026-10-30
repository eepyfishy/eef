# Continuous source inspection and planning

`eefn workloads sources` inspects the running local node. The coordinator owner
can inspect the same node with `eef node workload-sources --node NODE_ID`.
Both use backend operations; a dashboard or model is not required.

Inspection lists the applied microphone resources, their private-binding digests,
and the compiled-in energy adapter digest. It reports saved/applied microphone
permission, local listening consent/revision/epoch, whether saved resource
bindings still match the applied runtime, and whether Python is configured.
It does not import a device module, open a microphone, infer device availability,
choose a model, change preferences or authorize execution. Empty resources can
be prepared through the existing `eefn relay configure --microphone` command;
apply configuration with an explicit node restart.

Private device names/indices, adapter paths, resource parameters and properties
are omitted from the response. Unsupported private microphone bindings have no
usable digest. The same validator checks the actual local binding against a
reviewed definition. Device changes require a fresh review; a matching digest is
not a physical device identity or source ownership attestation.

Remote inspection uses the existing bounded control channel with independent
credentials and pinned TLS. The coordinator checks registration freshness,
runtime identity and the authenticated session before and after the read. The
node refuses this action over legacy shared-secret sessions. Older peers have
no fallback. The read-only operation uses reserved control capacity.

The existing explicit request compiler now accepts an exact continuous template
after immutable-origin/disclosure checks and a fresh authenticated source read.
The source/resource digests must match, and pending resource edits are refused.
Its checkpoint shares the existing task engine's shutdown gate. Retained retries
are read-only; compilation creates no approval, session offer or capture. The
node-facing planning view validates the exact nested source and task binding.
Legacy operation approval continues to refuse session plans.

Session execution remains unavailable. The compiled-in adapter bytes provide
the intended source identity; actual execution still needs a separately admitted
owned source, current local permissions/listening epoch, renewable authority,
resource/process quotas, bounded durable events and observed termination. The
ordinary Python streaming bridge still opens plugin paths and retains its
existing duration limit; this change does not make that API a pinned workload
runtime or silently extend it.
