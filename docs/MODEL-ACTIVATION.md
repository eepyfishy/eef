# Scoped automatic model activation (unreleased after alpha.6)

Owner-pinned setup can optionally restart the node runtime after configuration.
This is a separate consent from installation automation. Existing policies omit
restart_after_setup and remain manual; upgrades never opt them into restarting.
An approved default, full onboarding and real-model quality remain separate gates.

    eefn model-bootstrap-policy configure --mode auto --model CATALOG_ID --enable-automation --restart-after-setup
    eefn model-bootstrap-policy configure --manual-apply
    eef node model-bootstrap-policy --node NODE_ID configure --mode auto --model CATALOG_ID --enable-automation --restart-after-setup

This preference applies to a new eligible setup attempt, not retroactively to an
existing selection. Existing configured/applied models and command-line selections
remain protected. Model-specific command-line overrides require manual application
because a saved provider snapshot cannot override those arguments truthfully.

## Scope and admission

Automatic activation is a node **runtime restart**, not an application upgrade,
inference grant or memory/readiness guarantee. It can retire current runtime work
using the existing cancellation/owned-model shutdown path. Enable it only when
that interruption is acceptable. It neither widens permissions nor starts a
different application version selected by a pending update.

The controller retains the configuration actually used to build each runtime.
When setup saves its verified/provider-reported model and interpreter role, a
restart can be proposed only if unrelated runtime settings still match that
applied baseline. Setup preferences/catalog data and management consent are
already live node-service state, not pending runtime settings. Other fields,
including opaque future settings, stay in the comparison. A changed pending name,
permissions, endpoints, Python policy or other runtime setting blocks automatic
restart; model configuration remains saved for ordinary explicit application.

The runtime controller makes a final admission check against current policy,
runtime, selected model configuration, exact catalog pin and applied baseline.
Only then does it durably record draining and take the private startup snapshot.
Later owner edits cannot change that admitted restart's settings. Both the API
settings and replacement engine use the same captured configuration rather than
rereading the whole saved file between those steps. Nothing writes the captured
configuration back to disk. Late edits remain saved, unapplied and pending.

Admission is the boundary: turning automation off or editing settings before
admission prevents the restart. After admission, that one accepted restart may
finish; an opt-out does not reverse a runtime already being retired or unload the
model. The opt-out remains saved and is not rolled back by the frozen snapshot.
Use normal stop/restart and permission controls when further intervention is
needed. A separate ordinary explicit restart still applies current saved
settings and the existing selected-application handoff policy.

## Durable observations, not readiness

The existing bounded setup sidecar adds restart_requested, restart_draining,
runtime_configured and apply_blocked phases. Activation evidence binds the
expected model-configuration digest and, after engine binding, the replacement
runtime ID. A startup token is consumed once; cloned, cancelled, stale or already
consumed tokens cannot publish another runtime configuration. The snapshot is
not Debug/Serialize and is never included in a public report or durable record:
only sanitized IDs/digests and phase observations are exposed.

Runtime_configured means a fresh engine adopted the admitted configuration. It
does not prove model loading, successful inference, semantic quality or physical
memory release. readiness_verified remains false. Inspect model-lifecycle for
actual availability; owned GGUF process inventory and shared Ollama preparation
are different interfaces. Network show distinguishes saved and applied names.

The local status model_activation field is a sanitized current restart hint,
including draining/runtime_configured/unknown, without a private snapshot. The
setup journal remains the durable historical evidence. If evidence writes fail,
only the last durable checkpoint may be available; do not infer live execution
or successful rollback from it.

Generic restart/exit or unconfirmed shutdown clears queued/active tokens and
retains uncertainty. Cancellation serializes with configuration admission but
does not require readable saved settings: malformed JSON or an unavailable
optional journal cannot preserve the volatile token. It never repairs settings.
An interrupted process loses the in-memory token: the next
process marks unfinished setup unknown instead of reconstructing/replaying the
snapshot. An ordinary process launch still loads normal saved configuration,
just as before; that is not proof the earlier automatic restart completed.

## Verification

Both locked/offline workspace/all-target feature modes pass **928 tests plus one
ignored profiler each**: eef 268, owner CLI 21, installer 6, eefn 607 and node CLI
26. Eleven added regressions cover separate restart consent, CLI flag conflicts,
stale generic restoration, frozen late edits, one-shot startup, applied-baseline
and final policy/model/catalog/runtime fences, cancellation and live metadata.
The damaged-settings cleanup regression failed before the no-read serialization
fix and passes afterward. A separate queued/draining damaged-journal regression
confirms that missing durable evidence cannot preserve cancelled authority or
repair files. Logs are retained under:

- .validation/model-activation-cleanup-before.log (expected failure)
- .validation/model-activation-cleanup-after.log
- .validation/model-activation-workspace-default-final.log
- .validation/model-activation-workspace-headless-final.log

Final headless apps/examples build. Public model-activation-OUXuMs exercises
real local and pinned-TLS coordinator commands with synthetic providers: the
replacement engine advertises the exact interpreter, unrelated pending settings
and model CLI overrides block activation, later edits/opt-out remain saved and
pending, and an interrupted activation becomes unknown without token replay.
An isolated hard-linked fixture installation retains a pending application-version
marker during scoped activation. Its ordinary restart negative control consults
that marker and exits; the deliberately nonexistent target cannot launch another
executable. No installed application is mutated or copied.

Thirteen existing native suites also pass on that build:

- model-setup-K3Um28, model-bootstrap-edjxjU, unavailable-model-tPsJQO
- model-metadata-KslIRz, installed-selection-NjJ9F0, process-lifetime-s7sbrr
- local-pipelines-SAvNkI, python-policy-nSquu7, python-bridge-Wz2TVC
- permission-commands-L0TLd2, node-commands-gS4YLD, node-credentials-d2z6eU
- model-downloads-RaDiRE

Each fixture retains a public results.json (Python bridge uses result.json).
The combined native log is .validation/model-activation-native-final.log;
.validation/model-activation-final-native-hashes.json retains the four evaluated
application/helper hashes, all unchanged after acceptance. Source dependency
boundary, formatting, JS syntax and diff checks pass. Fixture apps/helpers and
llama-server are terminal.

These fixtures prove synthetic one-PC protocol/runtime behavior, not real model
weights, inference quality or physical two-PC acceptance. No release, installed
or remote node update, default model or visual UI change is implied. Approved
defaults, first-run dependencies, model quality, fresh physical testing and
installer/signing/Defender/stable-release gates remain open.
