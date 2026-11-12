# Network interpretation in normal input

This unreleased development integration passes complete hosted/source acceptance
at `3997e34`, followed by runtime-fence source `34d7993`. Exact runs and coverage
are recorded under validation scope below. This acceptance is separate from the
earlier explicit command and caller timing checkpoints. Published alpha
installers do not include these later changes.

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
or runtime attribution. Source `34d7993` passes all four jobs in
[run 42807522415](https://github.com/eepyfishy/eef/actions/runs/42807522415),
plus independent source/log acceptance.

New hosted regressions exercise normal model-free input through interpretation
and preparation, actual saved-question response through resolution/preparation,
saved-but-unapplied consent, failure without fallback/replay, origin/source
retention and rejection of altered receipts. CLI parsing and the existing native
command suite cover the isolated boolean policy patch and readback. These tests
use a private authenticated node pair and mock model backend. Initial integration
`3997e34` also passes [run 42806728479](https://github.com/eepyfishy/eef/actions/runs/42806728479)
and its independent source/log audit. The fence follow-up passes 1,734
Windows/headless Rust tests, one ignored across 17 groups, 37 native suites and
existing extra gates. These checks do not establish real-model quality, physical multi-PC behavior,
strong computation stop, complete onboarding or stable v0.4.0 readiness.

### Native command-path follow-up

The check extends `tools/test-request-review.mjs` with an isolated
two-logical-node workflow in `tools/network-input-command-workflow.mjs`. It uses
the source-built Windows executables, independent pinned-TLS credentials, the
public workflow switch and owner service-policy commands. The input node selects
no model; only the second node serves a fake loopback Ollama response. Exactly
two source-bound model requests interpret input and prepare a method targeting
the second node.

The check requires review before execution, refusal before owner approval,
explicit owner approval through commands, a successful remote `system.info`
grant, and result retrieval through the original input node. It compares the
origin's result with the executor's retained grant report, rejects executor
access to the origin-owned plan, and requires input/execution receipt reuse
without more inference or a changed execution record. No browser is loaded.

The existing native suite keeps its earlier cases and additionally requires the
new workflow to finish before recording success. Exact source/raw-log review of
`bff39d3` in [run 42944983996](https://github.com/eepyfishy/eef/actions/runs/42944983996)
confirms all four jobs and complete native gates pass, including both the new
workflow and original request-review success markers. It uses fake inference
and two logical nodes on one runner;
real-model quality and physical two-PC acceptance remain separate requirements.

The first command-path candidate `f635355` failed in
[run 42941420746](https://github.com/eepyfishy/eef/actions/runs/42941420746)
during method preparation; its Rust regression jobs passed. The fake method
supplied `constraints.node_id`, which the existing semantic validator forbids:
models may suggest `node_name` or `area`, while the compiler binds routing IDs.
The corrected fixture names its configured serving node and retains the exact
executor-ID, origin, approval, result and receipt-reuse assertions. Product
validation is unchanged. The failed run remains evidence, not a passing check;
the corrected fixture is accepted only through the complete later run above.
Intermediate source `0871daf` passed the Windows/headless command workflow but
failed its Windows/dashboard interpreter-recovery checkpoint wait in
[run 42944697616](https://github.com/eepyfishy/eef/actions/runs/42944697616).
That failed run is retained; the later pass does not establish its root cause.
