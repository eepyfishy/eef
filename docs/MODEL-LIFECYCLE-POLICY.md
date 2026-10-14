# Model lifecycle policy

Unreleased after alpha.6 and the [manual lifecycle checkpoint](MODEL-LIFECYCLE.md).
This extends the existing node service and runtime-owned model worker, not a
dashboard controller, second job runner or model inference authorization path.
The full stable 0.4.0 roadmap remains open.

## Owner commands

```text
eefn --json model-lifecycle-policy show
eef --json node model-lifecycle-policy --node NODE_ID show
```

Reports distinguish `saved_policy`/`saved_revision`/`saved_digest` from
`applied_policy`/`applied_revision` and the running `runtime_id`. Copy the exact current runtime and saved digest when
saving a running node's policy:

```text
eefn --json model-lifecycle-policy --runtime RUNTIME_UUID set --expected-digest SAVED_DIGEST --startup manual --idle-unload-seconds 120
eef --json node model-lifecycle-policy --node NODE_ID --runtime RUNTIME_UUID set --expected-digest SAVED_DIGEST --startup eager --idle-unload-seconds 0
```

Local policy setup also works while the node is stopped, under its exclusive
instance lock; omit `--runtime` only then. HTTP/RPC cannot claim offline mode.
Remote mutation needs current node-owner management approval; read-only status
does not. Saved policy changes preserve unrelated configuration. No model,
provider, role, artifact, inference permission or update setting is selected.

Saving never sends a restart. Use the existing explicit node restart command
to apply changed startup/limits/idle settings, or start a stopped node normally.
Reports state `restart_required` while a running node has a saved/applied
mismatch. A saved mismatch prevents new automatic idle withdrawals immediately,
even before restart; it does not undo a withdrawal already begun or stop active
inference. New manual load/unload admissions also require an applied matching
policy. Read-only receipt/status inspection remains available.

The saved digest binds the canonical normalized policy and persisted revision,
not a secret or approval token. Every actual policy change advances the
backend-owned `model_lifecycle_revision`, including generic saves and config
restores. Restoring identical old values neither revives an old CAS request nor
reactivates an old runtime's idle policy: a new explicit restart is still needed.
Normal callers cannot lower the revision in a config save. Revision exhaustion
never wraps; malformed saved revisions are retained and fail validation rather
than resetting to defaults. Direct out-of-band rewriting of application state is
outside this command-history guarantee; preserve configuration backups.
Stale digests and stale runtimes reject mutation. After an
unconfirmed reply inspect saved policy before a new owner decision; no client
retries, implicitly refreshes a fence, switches provider or falls back offline.
There is no durable policy-command receipt ledger. An unchanged command with
the current digest performs no write; the old digest cannot replay a changed
save or authorize execution. Origin guards protect the local command endpoints.

## Settings and compatibility

The optional `model_lifecycle` node configuration uses these validated fields:

| Field | Default | Accepted choices |
| --- | --- | --- |
| `startup` | `eager` | `eager` starts configured GGUF selections; `manual` leaves them unloaded until an explicit load; `on_demand` leaves them cold until an authorized inference/owned preview needs them. CLI spelling: `on-demand`. |
| `idle_unload_seconds` | null (disabled) | 1-86400 seconds; command value zero saves null/disabled. |
| `queue_capacity` | 8 | 1-64 queued manual model operations. |
| `receipt_capacity` | 256 | 1-4096 retained manual receipts, at least queue capacity. No replay-protection eviction. |
| `load_timeout_seconds` | 300 | 1-3600 seconds for configured group startup, explicit and demand load health readiness. Existing request deadlines can end demand loading earlier. |
| `termination_wait_seconds` | 5 | 1-30 seconds for explicit/idle owned-root termination wait. |
| `ollama_keep_alive_seconds` | null (inherit backend) | Ollama inference request preference: -1 indefinite, 0 request unload after its response, or 1-86400 seconds. Not ownership or observed residency. |
| `thinking_mode` | inherit (omitted on the wire) | Node-wide inference hint: inherit, disabled or enabled. Not observed thinking state or per-model capability discovery. |

Each numeric field has the corresponding hyphenated `set` option. Missing
policy fields retain defaults; malformed/null policy objects, unknown fields,
unbounded values and unsupported startup choices fail validation before use.
Older configurations remain eager with idle unloading disabled. These limits
bound model control, not CPU percentage, physical memory, VRAM or job grants.
The separate node work/reserved-control request limits still apply.

Manual startup is not on-demand loading. A manually started or idle-unloaded
model still needs an explicit load unless the owner selects `on_demand`.
On-demand policy advertises eligible cold selections as `unloaded`/`available`,
not ready. Status, compilation, approval and heartbeat do not load anything.
Actual inference uses the existing model-server operation lock, with current
runtime/selection/policy checks at native creation and readiness confirmation.
The owned root must still be live at handoff; a successful HTTP health response
alone cannot turn an already-exited owned process into readiness.
The load is owned by the requesting future, not a detached resource job. Its
health readiness transfers atomically to an active inference pin before idle
withdrawal can race it. Already-admitted concurrent requests can share that load
and receive separate pins; ordinary node work/preview admission still bounds
their lifetimes. Long loads consume the existing request/grant deadline, never
extend it. Cancelled/failed partial loads are cleaned and become error, not
eligible for implicit retry. Explicit owner repair/load/restart is required.
No provider switch, download, selection or grant authorization is added.
Default behavior is unchanged; this is an optional resource preference, not
completed bootstrap or measured startup-cost/resource optimization.
Startup/idle/native-control settings govern owned GGUF processes, not the user's
shared Ollama service. The separate Ollama request preference below does not
implement exclusive/shared ownership, observed residency or autonomous unload.

### Interpreter sampling preferences (unreleased)

```text
eefn --json model-lifecycle-policy show
eefn --json model-lifecycle-policy --runtime RUNTIME_UUID set --expected-digest SAVED_DIGEST --interpreter-temperature 0.7 --interpreter-seed 42
eef --json node model-lifecycle-policy --node NODE_ID --runtime RUNTIME_UUID set --expected-digest SAVED_DIGEST --interpreter-sampling-default
```

Sampling is a node-wide preference for the one selected request interpreter,
including clarification and method preparation, not a per-model override or
generic reviewed workload parameter. The same owner approval, exact runtime,
revision/CAS, saved/applied and explicit-restart rules apply. Saving, inspecting
or resetting performs no inference/download/model selection or restart. Claimed
applied settings must match the engine before runtime replacement is accepted.

| CLI option | Bounds | Backend option |
| --- | --- | --- |
| `--interpreter-temperature` | 0-2, up to three fractional places | `temperature` |
| `--interpreter-top-p` | 0.001-1 | `top_p` |
| `--interpreter-top-k` | integer 0-1024 | `top_k` |
| `--interpreter-min-p` | 0-1 | `min_p` |
| `--interpreter-presence-penalty` | -2 to 2 | `presence_penalty` |
| `--interpreter-repeat-penalty` | 0.1-2 | `repeat_penalty` |
| `--interpreter-seed` | integer 0-4294967294 | `seed` |

Default interpretation remains temperature zero; optional hints are omitted so
backend defaults apply. Integer thousandths in `interpreter_sampling` preserve
exact saved equality/digests; CLI users provide ordinary decimals. Fields omitted
from a patch keep their saved values. `--interpreter-sampling-default` resets the
whole group and conflicts with explicit sampling choices, including on the wire.
The all-default group and empty command patch are omitted from serialized policy/
commands, preserving previous canonical digests and legacy reports. Individual
optional-field reset/per-model sampling remains future work.

Both adapters map these hints to their existing backend request format (Ollama
`options`, llama.cpp top-level fields). Caller/model text cannot change the applied
interpreter setting. Ordinary reviewed `llm.infer`/`vlm.analyze` retains its
existing parameter behavior; it does not acquire these private interpreter
options. Prompts, schema, strict validator, token/byte/deadline bounds, permissions,
role selection and no-fallback/no-retry rules are unchanged. A seed is a backend
hint, not proof of reproducibility across implementations, models or hardware.
No named-model profile is automatically selected, and no quality/default approval
is implied by successful configuration or inference.

### Thinking request preference (unreleased)

```text
eefn --json model-lifecycle-policy show
eefn --json model-lifecycle-policy --runtime RUNTIME_UUID set --expected-digest SAVED_DIGEST --thinking-mode disabled
eef --json node model-lifecycle-policy --node NODE_ID --runtime RUNTIME_UUID set --expected-digest SAVED_DIGEST --thinking-mode inherit
```

`inherit` (default), `disabled`, and `enabled` are owner-selected request hints,
not model capability discovery or proof of the backend's actual thinking state.
The setting currently applies to the node's selected models, including local
request interpretation. Per-model overrides and supported-value discovery remain
future work. Unsupported backends/models may reject or ignore the hint; errors
are retained without fallback, automatic retry, or token-budget increases.

Use the inspected saved digest and, on a running node, its exact runtime ID.
Saving uses existing local-owner/approved-remote management and revision fences;
it never loads/downloads/selects a model, sends inference, or restarts the node.
The running engine retains its old preference until an explicit restart. Runtime
binding rejects a report claiming a preference not present in the engine without
closing the existing runtime. Restoring `inherit` omits the hint; inherited policy
serialization/digests and old command wire reports remain unchanged.

The engine maps explicit choices to Ollama's [`think` Boolean](https://docs.ollama.com/api/chat)
and llama.cpp's [`chat_template_kwargs.enable_thinking`](https://github.com/ggml-org/llama.cpp/blob/c1d0e7a00/tools/server/README.md#post-v1chatcompletions-openai-compatible-chat-completions-api).
For llama.cpp, disabled also sends `reasoning_effort:"none"`. It does **not**
send `reasoning_format:"none"`, which changes parsing rather than disabling
thinking. Model names are not special-cased. Caller/model-supplied parameters
cannot override this applied owner preference. Prompts, schema, result validation,
token/byte/time limits, execution permission and partial-output metadata are
unchanged; reasoning output is not promoted to final text.

### Shared Ollama request preference

```text
eefn --json model-lifecycle-policy --runtime RUNTIME_UUID set --expected-digest SAVED_DIGEST --ollama-keep-alive-seconds 120
eef --json node model-lifecycle-policy --node NODE_ID --runtime RUNTIME_UUID set --expected-digest SAVED_DIGEST --ollama-keep-alive-default
```

These choices use the same exact node/runtime, CAS revision, local owner and
approved remote-management checks as other policy settings. The duration and
reset options conflict, including on the wire. Zero means an Ollama post-response
unload preference, unlike the owned-GGUF idle option where zero disables unloading.
Negative values other than -1 and durations above one day are rejected. Restoring
the backend default saves null; it does not send a backend command.

Saving never contacts Ollama or restarts the node. Until explicit restart,
inference uses the old applied preference; the new runtime uses the saved one.
Runtime binding verifies that the claimed applied preference matches the engine;
a mismatch rejects replacement without closing the existing runtime.
The engine constructs the backend request from this applied owner setting, not
caller/model-supplied `keep_alive` parameters. Read-only policy, compilation,
approval and result inspection never send inference or empty preload/unload.

Previously EEFN always sent `keep_alive:-1`, forcing indefinite residency. New
unreleased code intentionally omits the hint by default so the backend owner’s
policy applies. Select -1 explicitly if that retention is wanted. Published
alpha.6 behavior/artifacts are unchanged. Older configs normalize to the new
inherit choice; fresh policy inspection is required because canonical digests
include the new normalized field. Old CAS values do not silently gain new scope.

Ollama documents numeric seconds, negative indefinite retention, zero after-response
unload, and omission inheriting its server settings in its [FAQ](https://docs.ollama.com/faq)
and [chat API](https://docs.ollama.com/api/chat). This is a preference submitted only
with ordinary authorized inference/owned preview. It can affect shared cache
residency; another application may change it and EEF cannot promise a release
time or exclusive ownership. Cancellation of a client future does not prove
shared backend computation stopped. No service termination, environment edit,
model deletion/download, background unload loop or physical-memory guarantee is
added. Owned lifecycle commands still truthfully report Ollama unsupported.
Full ownership/coexistence and observed-resource acceptance remain required.

## Idle behavior and observability

Elapsed inactivity uses a monotonic clock. Successful readiness starts the
clock; taking an inference pin clears it; the final pin release starts a fresh
warm-idle interval. Inspecting status, GUI polling and heartbeat traffic do not
count as inference activity and do not keep a model resident.

The existing model worker owns the conditional one-second idle poll. It
prioritizes queued manual operations, skips missed ticks, and never catches up
or loads a model automatically. A long model operation can delay idle cleanup;
the timeout is a minimum inactive interval, not an exact OS exit deadline.
No idle polling branch runs when idle unloading is disabled.

Candidates are rechecked against generation, elapsed inactivity, active pins
and pending control under the same gate that admits inference. A recently used
model cannot be unloaded using an earlier candidate snapshot, even when its
process generation is unchanged. Reserved idle withdrawal blocks new pins.
Current node identity, runtime ownership, selected model settings and applied
policy are checked again under configuration authority at application.

Idle cleanup uses the existing bounded owned-process/tree unload path, without
deleting model files, issuing grants or restarting the node. It records up to
32 runtime-local `idle_events` in lifecycle status, each with model ID, original
generation and completed/failed outcome. These diagnostic events do not consume
the manual receipt budget and are not durable recovery/migration proof. They
do not measure physical RAM/VRAM release; interrupted or failed work is never
converted into fabricated success or implicitly replayed.

Policy status uses bounded reserved network control capacity; policy mutation
uses ordinary work admission. The reserved action rejects substituted `set`
commands. Headless/browser-disabled and offline-owner paths share this service.

## Verification scope

`tools/test-model-lifecycle-policy.mjs` exercises locked offline setup, unchanged
defaults, manual no-process startup, pinned-TLS EEF control, remote owner approval,
saved/applied separation, stale digest refusal, explicit restart, active inference
pins across the idle interval, reset after the last pin, automatic inactive unload,
historical load no-replay, reserved status under work saturation, immediate
saved-policy pause, rollback without CAS/idle-policy revival, and explicit eager
startup with idle unloading disabled.
It uses a synthetic backend on one PC, not real interpreter quality or physical
two-PC acceptance. Existing manual lifecycle/startup and real-GGUF fixtures must
also stay passing. Resource measurement, measured startup-cost routing, shared
Ollama controls, measured startup-cost/resource optimization, broader workload/
mesh/cluster scope and release acceptance remain.

`tools/test-ollama-residency-policy.mjs` verifies the separate shared request
preference using two headless roles, independently provisioned credentials,
pinned TLS and a synthetic HTTP backend. It covers offline save without backend
contact, inherited/default omission, current remote owner approval, CAS refusal,
saved/applied separation, explicit restart, reviewed requests with numeric hints,
default restoration, no background preload/unload and truthful unsupported owned
lifecycle. It is not actual Ollama coexistence or physical memory acceptance.

## 2026-10-13 shared-request policy verification checkpoint

Final workspace/all-target suites pass **571 tests** in each feature mode:
EEF library 220, owner CLI 16, installer 6, EEFN library 318, node CLI 11.
Fresh default workspace binaries/examples, formatting/diff/JavaScript syntax
and the backend/browser dependency guard pass. New units cover normalized older
configs, strict numeric bounds, duration/default conflicts on the wire and CLI,
CAS/default restoration, substituted policy confirmation, caller-independent
payload construction, invalid engine preferences and applied-policy/engine
consistency without closing a valid previous runtime on rejected replacement.

Final-binary `ollama-residency-ZywaF2` passes the public shared-preference path.
Model metadata/selection `model-metadata-NcfkRE`, owned policy/on-demand
`model-policy-K98jld`, cached real CPU inference/lifecycle `real-model-HVdDZX`
and manual lifecycle `model-lifecycle-56LGBs` also pass; the full five-fixture
chain exits **0**. The real test uses the existing exact-size/hash-verified 1.5B
artifact and cached alpha.6 runtime without downloads/copies. It proves the
owned-GGUF regression path, not actual shared Ollama residence, interpreter
quality, physical memory release or physical two-PC operation.

The initial compile failed because a new node CLI test lacked test-only
`serde_json` imports; those imports were added without production changes.
An atomic documentation patch failed on mismatched context, made no changes and
was reapplied against actual content. Intermediate both-mode suites passed 570
tests and `ollama-residency-xZ5D6D` passed public commands before the final
runtime-binding consistency guard; these are not final-guard acceptance evidence.

This increment does not finish full Ollama ownership/lifecycle, per-model
resource policy, measured routing, bootstrap quality, general pipelines/media,
authenticated peer transfer, mesh/cluster or physical/versioned release gates.
No installed app, physical node, release/feed, UI asset, real shared Ollama
service or signing/Defender acceptance is changed by this source checkpoint.

## 2026-10-13 on-demand verification checkpoint

Final workspace/all-target suites pass **563 tests** in each feature mode:
EEF library 220, owner CLI 15, installer 6, EEFN library 312, node CLI 10.
Fresh default workspace binaries/examples, formatting/diff/JavaScript syntax
and the backend/browser dependency guard pass. Tests use serial execution to
avoid oversubscribing real-clock authorization fixtures on this machine.

New units cover read-only cold availability without spawning, saved-policy
revision and runtime closure withdrawing demand authority, failed-load no-retry,
concurrent waiters sharing one owned load with separate atomic active pins,
current authority refusal after health readiness, and an owned root exiting
before handoff without fabricated readiness. Explicit resident metadata feeds
the existing warm routing score; legacy unknown residency remains unloaded.
This is not measured startup-cost or memory-aware admission.

Final-binary public `model-policy-ZcUfgW` passes existing policy/CAS/idle behavior
and on-demand cold availability, approval/status without loading, reviewed
request load, reload after idle, and expired partial-load cleanup with persisted
unknown outcome and no implicit retry. `real-model-H1Jx9L` passes real CPU demand
inference using the existing exact-size/hash-verified 1.5B artifact and cached
alpha.6 runtime: cold startup, request-owned load, completion, subsequent idle
unload without a node restart, and unchanged completed history. No model
download/copy is made; physical two-PC operation, interpreter quality and
physical RAM/VRAM release are not proved.

Fresh public manual lifecycle `model-lifecycle-1rRWfI` and non-blocking group
startup `model-startup-haMn9P` also pass. The complete policy/real-model/manual/
startup fixture chain exits **0**, not merely the individual intermediate steps.
Final-source regressions pass model metadata/selection `model-metadata-XaEOGd`,
automatic scheduling/refusal `automation-schedules-gZJHBU`, manual occurrence/
archive `automation-occurrences-wUEyTP`, node input/answers `request-input-dOfcJF`
and reviewed execution/recovery `request-review-PWZ7Qo`; that full chain also
exits **0**. Reviewed regression inference is synthetic, not interpreter quality.

An initial native coalescing unit used a synthetic HTTP fixture that closed
without draining the incoming request; it timed out. The test server now reads
the request before replying, and the final-authorization refusal test verifies
that it actually reaches that check rather than passing through timeout alone.
An initial full suite also expired an archive/no-refund test's short approval;
only that fixture now uses a longer bounded validity. Default expiry fixtures
and production authorization/deadline/recovery behavior remain unchanged.
Failed runs are not acceptance evidence. An atomic documentation patch failed
on mismatched context, made no changes and was reapplied against actual content.

Ollama lifecycle, per-model resource policy, measured startup costs, bootstrap
quality, full pipelines/media/mesh/cluster and physical/versioned release gates
remain required. No installed app, physical node, release/feed, UI asset or
signing/Defender acceptance is changed by this source checkpoint.

## 2026-10-12 verification checkpoint

Final workspace/all-target suites pass **557 tests** in each feature mode:
EEF library 219, owner CLI 15, installer 6, EEFN library 307, node CLI 10.
Fresh default binaries/examples, formatting/diff/JavaScript syntax and the
backend/browser dependency guard pass separately. The new unit cases cover
policy bounds/digests, locked offline CAS, running owner/runtime fences,
saved/applied state, substituted report authority, monotonic idle pin rechecks,
owned-root cleanup, no manual receipt consumption, disabled/mismatched policy,
generic restore without revision rollback, exhausted/corrupt revision retention,
and identical-content rollback without old-runtime idle revival.

Final `model-policy-KFeRMP` passes public headless/pinned-TLS policy behavior,
including actual preserved owned residency across an identical-content rollback
and refusal of the original old CAS. `model-lifecycle-ValZbF` and
`model-startup-Aujcmg` pass manual lifecycle and existing group startup behavior.
The complete policy/lifecycle/startup/real-model fixture chain exits **0**.

`real-model-dBHZ0L` passes with the existing size/hash-verified 1.5B GGUF and
cached alpha.6 runtime: manual unload/reload, reviewed CPU inference afterward,
saved-not-applied idle policy, explicit restart to apply it, then actual automatic
idle unload without another node restart or artifact deletion. No model download
or copy is made. Interpreter quality, physical two-PC operation, physical memory
release, CPU-percent/VRAM quotas and stable-release acceptance are not proved.

The initial full-suite schedule test failed its expected refusal text: its short
test policy could reach expiry margin before spent-run budget was checked.
Its fixture now declares a longer bounded test validity and separately asserts
both spent-budget and near-expiry refusal. No production authorization, expiry,
allowance, chronology or recovery rule was relaxed. A failed atomic source patch
made no changes and was reapplied against the actual file content.

An earlier chained metadata run `model-metadata-shgW37` exited with unexplained
native code `0xc0000409`; no assertion/terminal pass evidence establishes it as
passing. The separate rerun `model-metadata-18ta1x` exits 0. That unexplained run
is retained, not silently counted or attributed to a proven cause. Final-source
regression evidence is recorded separately below. No installed app, physical
node, release/feed, UI asset or signing/Defender acceptance is changed here.

Final-source model metadata `model-metadata-Hjv37Q` exits 0. The subsequent
scheduling/refusal `automation-schedules-WKd6ru`, native lifetime
`process-lifetime-48InJA`, node input/answers `request-input-dSxG18`, and reviewed
execution/recovery `request-review-jhynem` fixtures also pass, with the complete
regression chain exiting 0. These fixtures remain isolated one-PC acceptance,
not the missing physical-network, interpreter, pipeline or stable-release gates.
