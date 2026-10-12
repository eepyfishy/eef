# Owned-model lifecycle policy

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
| `startup` | `eager` | `eager` starts configured GGUF selections as before; `manual` leaves them unloaded until an explicit load command. |
| `idle_unload_seconds` | null (disabled) | 1-86400 seconds; command value zero saves null/disabled. |
| `queue_capacity` | 8 | 1-64 queued manual model operations. |
| `receipt_capacity` | 256 | 1-4096 retained manual receipts, at least queue capacity. No replay-protection eviction. |
| `load_timeout_seconds` | 300 | 1-3600 seconds for configured group startup and explicit load health readiness. |
| `termination_wait_seconds` | 5 | 1-30 seconds for explicit/idle owned-root termination wait. |

Each numeric field has the corresponding hyphenated `set` option. Missing
policy fields retain defaults; malformed/null policy objects, unknown fields,
unbounded values and unsupported startup choices fail validation before use.
Older configurations remain eager with idle unloading disabled. These limits
bound model control, not CPU percentage, physical memory, VRAM or job grants.
The separate node work/reserved-control request limits still apply.

Manual startup is not on-demand loading. Until cold-model routing is implemented,
an owner must explicitly load a manually started or idle-unloaded model before
it can serve inference or request interpretation. Enabling idle unloading may
make an interpreter unavailable until that explicit load. Default behavior is
unchanged; this is an optional resource preference, not completed bootstrap.
The policy controls owned GGUF processes only, not the user's shared Ollama
service. Ollama lifecycle controls remain separate required work.

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
also stay passing. Resource measurement, on-demand/startup-cost routing, shared
Ollama controls, broader workload/mesh/cluster scope and release acceptance remain.

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
