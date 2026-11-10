# Network interpretation in normal input

This is an unreleased development follow-up awaiting its own hosted validation.
The earlier accepted explicit network command and caller timing checkpoints do
not accept this integration. Published alpha installers do not include it.

An input node can opt into using an owner-authorized interpreter service for
initial interpretation, clarification answers and method preparation. The input
node needs no installed model; ordinary status, configuration, explicit commands
and recovery remain available without any interpreter. No new dependency,
background supervisor, idle polling or model installation is introduced.

## Choose where interpretation happens

Existing and fresh configurations default `input_workflow.network_interpreter`
to `false`, preserving local interpretation. To opt into network processing:

```text
eefn requests workflow set --network-interpreter true
eefn requests workflow show --json
eefn restart --wait-seconds 60
eefn requests input --text "Inspect node status"
```

As with the existing input policy, saving a preference does not change the
running node. Inspect `saved_policy`, `applied_policy` and `restart_required`.
`--network-interpreter false` returns to local interpretation after application.
This switch grants no provider permission and does not select or download a
model. The provider needs an applied sharing permission and selected interpreter;
the coordinator owner must approve the origin/provider service policy described
in [network interpreter commands](NETWORK-INTERPRETER-COMMANDS.md).

Network mode routes these interpretation stages through that service even if
the input node has a local model. Local mode never automatically falls back to
a peer after local inference fails. Explicit `interpret-network` remains a
separate owner-invoked command with its own provider/local-only restrictions.
`interpret-saved` uses applied input policy; offline preview remains local.

Opting in permits sending the saved text, relevant answers and disclosed
capability context to the authorized executing node. That node's operator can
see the supplied context. Encryption does not hide it from the executor. Users
who require local interpretation should keep this preference off.

## Same durable workflow and authority

The origin node saves raw input first. A remote interpreter does not become its
origin or replace its area/context. Initial results carry the coordinator's
actual source-bound checkpoint. Clarification uses a real scoped draft read
after source application; method preparation uses the existing planning view
and checkpoint validation. No node fabricates an opening or grants local-model
authority to a remote receipt.

Normal input and answer workflows borrow their existing input admission slot
through the broker. Separate commands acquire that same capacity. Each stage
preserves its operation ID, source revision, actual provider/model, bounded grant
and caller timing. There is no additional queue or provider substitution.
The new receipt branches coexist with the unchanged local wire claim/promotion
guards. Reports reject remote inference attributed to a local interpreter or
remote workflow receipts under a local-only applied policy.

Missing service permission, provider failure, model rejection, cancellation or
an unconfirmed reply retains the source and available evidence. Repeated outer
receipts inspect history; they do not infer again or continue uncertain work.
Changing source between a remote result and the subsequent draft read fails
confirmation. Model output still needs deterministic validation, and resulting
tool work still needs separate owner review, approval and execution.

## Validation scope

The runtime-fence review follow-up observes retirement through the entire
network preparation/resolution branch, including the final scoped draft read.
Errors clear success flags while preserving uncertain saves. Applied receipts
must describe a final attempt, and fresh initial checkpoints must name the
requesting node runtime. Additional regression coverage retires an actual
model-free input workflow during a held service call and rejects altered phase
or runtime attribution. This follow-up also awaits fresh hosted acceptance.

New hosted regressions exercise normal model-free input through interpretation
and preparation, actual saved-question response through resolution/preparation,
saved-but-unapplied consent, failure without fallback/replay, origin/source
retention and rejection of altered receipts. CLI parsing and the existing native
command suite cover the isolated boolean policy patch and readback. These tests
use a private authenticated node pair and mock model backend. Hosted acceptance
is pending; they do not establish real-model quality, physical multi-PC behavior,
strong computation stop, complete onboarding or stable v0.4.0 readiness.
