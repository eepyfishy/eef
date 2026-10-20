# One-attempt model setup approval (unreleased after alpha.6)

Enabled prompt mode can retain explicit consent for one setup attempt without
switching the node to auto mode. This uses the existing node-owned installer,
receipts, policy/runtime/model/catalog fences and setup journal. It neither
endorses a project default nor approves the selected model's semantic quality.

    eefn model-bootstrap-policy configure --mode prompt --model CATALOG_ID --enable-automation
    eefn model-bootstrap-policy show
    eefn model-bootstrap-policy approve
    eefn model-bootstrap-policy show

The same operation is available through an approved coordinator connection:

    eef node model-bootstrap-policy --node NODE_ID approve

Current local remote-management consent is required for remote approval; a
coordinator cannot enable that consent itself. The client inspects the exact
current policy and runtime before requesting approval. Neither CLI nor dashboard
owns the backend loop. No JSON editing, port numbers or browser are needed.

## Consent and lifecycle

Approval requires enabled prompt mode, one unchanged owner catalog pin and no
existing saved/applied model selection. It records one `approved` attempt before
any asynchronous provider inspection. The retained evidence binds the node,
runtime, complete versioned policy and current model configuration digest;
`owner_approved: true` is immutable within that attempt. Old auto records omit
the new flag and keep their previous representation. Auto mode does not consume
prompt consent or require this command.

The client compare-and-swap binds its inspected policy (including the complete
catalog pin) and runtime. Model settings are captured under the node's lock when
approval is recorded, not reserved by the earlier inspection. Approval is not a
provider/preflight quote; provider availability and disk checks still occur in
the backend. Subsequent model-settings changes invalidate this retained consent.

The approval call does not change configuration bytes, policy mode/revision,
permissions, selections, connection settings or update policy. Its `changed`
field reports the durable approval write, not a configuration write. The
runtime-owned supervisor separately consumes approval into `inspecting` and the
existing installation/selection lifecycle. A busy installer leaves consent
queued without another probe or transfer; the supervisor rechecks its scope
before consumption and around asynchronous work. Approval of an installed model
may only reuse its reported metadata; otherwise the existing installer performs
the single identified transfer.

Disabling automation or changing policy, selected catalog data, model settings
or runtime invalidates unconsumed consent. A rejected or uncertain request never
causes client fallback/replay. Inspect `show` and its exact installer receipt.
One policy epoch cannot acquire a second attempt, even after model deselection,
failure or interruption. Repeated approval is refused rather than silently
starting another operation. After inspecting the earlier outcome, an owner may
explicitly disable/re-enable automation to create a new epoch and then decide
whether to approve it; this is not automatic retry or proof of rollback.

Process restart marks old unfinished consent/attempts unknown; it does not replay
their approval. A consent recorded for the current runtime just before its setup
supervisor starts remains valid for that runtime. Corrupt/future/oversized optional
evidence refuses approval without initialization or repair, preserving settings
and deterministic core controls.

## Restart remains separate

The default is still manual application after configuration. The existing
`--restart-after-setup` option can independently authorize a [scoped runtime
restart](MODEL-ACTIVATION.md) after this one approved attempt. Without it, inspect
pending settings and use an ordinary explicit restart. Prompt mode and its policy
epoch remain unchanged in either path. Runtime configuration is not model
loading, successful inference, work approval or readiness.

Older automation reports remain readable. An older node that does not support
`approve` must reject it; the client does not replace it with a mode change or
legacy settings save. A legacy explanatory note cannot confirm a new approval.

## Verification

Eleven added Rust regressions cover node and coordinator command admission,
current remote consent, stale policy/runtime/catalog refusal, immutable consent,
busy transfers, revocation before and during metadata inspection, startup/crash
recovery, damaged evidence, repeat refusal, separate activation consent and legacy
report notes. Both locked/offline workspace/all-target feature modes pass **939
tests plus one ignored profiler each**: EEF 269, owner CLI 21, installer 6, EEFN
616 and node CLI 27. Final logs/build:

- .validation/prompt-approval-workspace-default-accepted.log
- .validation/prompt-approval-workspace-headless-final.log
- .validation/prompt-approval-build-accepted.log

Final headless apps/examples build. Expanded model-setup-2u0qTx passes real local
and pinned-TLS remote commands: prompt remains inactive before approval, current
remote consent is required, pending transfer leaves configuration bytes unchanged,
one synthetic pull exposes progress and its installed receipt, the backend selects
the exact model without changing prompt policy/epoch, repeat approval cannot start
another transfer, and no inference or automatic restart occurs. Existing automatic
setup, interrupted-process uncertainty/no replay, CPU cache, CLI-choice protection
and reviewed core-work cases still pass.

Expanded model-activation-A7g1g5 also passes prompt approval followed by the
separately opted-in scoped runtime restart. It preserves prompt policy/epoch and
binds the replacement engine without claiming readiness. The existing pending
settings, late edits/opt-out, application-version negative control, overrides and
crash-uncertainty cases remain intact.

The twelve other native suites pass on those same binaries:

- model-bootstrap-dRMHmH, unavailable-model-6KIXz5, model-metadata-o2qMdG
- installed-selection-DZhJOT, process-lifetime-h84QI2, local-pipelines-oFwLxO
- python-policy-ubkB2b, python-bridge-i5Seh5, permission-commands-tUte2b
- node-commands-AyuRJp, node-credentials-EeeQ2y, model-downloads-QIy8O6

Each fixture retains results.json (Python bridge uses result.json). The combined
native log is .validation/prompt-approval-native-final.log. Frozen changed Rust/JS
and Cargo source hashes are retained in .validation/prompt-approval-source-hashes.json;
the four evaluated application/helper hashes are in
.validation/prompt-approval-final-native-hashes.json. All match after acceptance.
Source dependency boundary, formatting, JS syntax and diff checks pass. Fixture
apps/helpers and llama-server are terminal.

These fixtures use synthetic providers/cache metadata, not real weights,
semantic quality or physical two-PC acceptance. No default model, installed or
remote node update, installer, visual UI, version or update-policy change is
implied. Full first-run dependencies, approved defaults, model quality, physical
testing and installer/signing/Defender/stable-release gates remain open.
